import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import { gitClone, gitCloneCancel, type ProbeResult } from "../lib/git";
import { checkPathExists } from "../lib/project";
import { joinPath, parentPath } from "../lib/paths";
import { getGeneralPrefs, setGeneralPrefs } from "../lib/prefs";
import { formatGitBusyLabel, useGitProgress } from "./useGitProgress";

/** `git@host:group/repo.git` -> `repo`. Carried over verbatim from the
 * component — the parsing is load-bearing for the destination path. */
function getRepoName(url: string): string | null {
  const trimmed = url.trim();
  if (!trimmed) return null;
  const withoutGit = trimmed.endsWith(".git") ? trimmed.slice(0, -4) : trimmed;
  const last = withoutGit.split("/").pop() ?? "";
  if (!last) return null;
  const colonIdx = last.lastIndexOf(":");
  return colonIdx >= 0 ? last.slice(colonIdx + 1) : last;
}

/** How long a clone may report nothing at all before we tell the user it
 * looks stuck. Long enough that a slow but healthy connect doesn't trip it. */
const STALL_HINT_MS = 60_000;

/** Everything the clone dialog does: remembering where the user last cloned
 * to, warning before overwriting an existing folder, and the clone itself.
 *
 * `needsAuth` is a distinct outcome rather than just another error message:
 * `no_ssh_credentials:` means the clone never started, and the dialog offers
 * a jump to settings instead of asking the user to read a failure. */
export function useCloneRepo(onOpened?: (project: ProbeResult) => unknown) {
  const [url, setUrl] = useState("");
  const [baseDir, setBaseDir] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [cloning, setCloning] = useState(false);
  const [needsAuth, setNeedsAuth] = useState(false);
  const [conflict, setConflict] = useState(false);
  const gitProgress = useGitProgress();
  /** Bumped by `cancel` so a late answer from an abandoned clone is ignored.
   * The backend cannot always be stopped — a thread wedged inside a blocking
   * syscall never reaches a libgit2 callback — so the UI stops waiting on its
   * own rather than treating cancellation as a handshake. */
  const runRef = useRef(0);
  const cloneIdRef = useRef<string | null>(null);
  const [stalled, setStalled] = useState(false);

  const repoName = useMemo(() => getRepoName(url), [url]);
  const destination = repoName ? joinPath(baseDir, repoName) : baseDir;
  /** Caption for the submit button while the clone runs — already complete,
   * so the component does not compose it with a verb of its own. */
  const busyLabel = cloning
    ? formatGitBusyLabel("Клонирование", gitProgress.event)
    : null;

  /** A clone that produces no progress event at all is the failure mode this
   * whole flow exists to make legible — say so rather than spinning silently.
   * Only a hint: nothing is aborted automatically. */
  useEffect(() => {
    if (!cloning) {
      setStalled(false);
      return;
    }
    setStalled(false);
    const timer = setTimeout(() => setStalled(true), STALL_HINT_MS);
    return () => clearTimeout(timer);
  }, [cloning, gitProgress.event]);

  useEffect(() => {
    getGeneralPrefs()
      .then((prefs) => {
        if (prefs.lastCloneDir) setBaseDir(prefs.lastCloneDir);
      })
      // No remembered folder is the normal first-run state, not a failure.
      .catch(() => {});
  }, []);

  /** Debounced so typing a URL doesn't hit the filesystem per keystroke. */
  useEffect(() => {
    if (!destination) {
      setConflict(false);
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      void checkPathExists(destination).then((result) => {
        // Only a *non-empty* directory blocks the clone — that is the rule the
        // backend enforces. An empty leftover folder from an aborted attempt
        // must not lock the user out of retrying into the same path.
        if (!cancelled) setConflict(result.isNonEmpty);
      });
    }, 400);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [destination]);

  /** The field shows `baseDir + repoName`, so an edit has to be split back
   * apart. `parentPath` handles Windows separators — a `C:\repos\x` path has no
   * forward slash at all, and naive splitting used to leave the whole value as
   * the base dir and append the repo name a second time. */
  const setDestination = useCallback((value: string) => {
    setBaseDir(parentPath(value));
  }, []);

  const pickDestination = useCallback(async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Папка для клонирования",
    });
    if (selected === null || Array.isArray(selected)) return;
    setBaseDir(selected);
    try {
      const current = await getGeneralPrefs();
      await setGeneralPrefs({ ...current, lastCloneDir: selected });
    } catch {
      // Persistence failure is not worth interrupting the clone over — the
      // selection still applies for this session.
    }
  }, []);

  const submit = useCallback(async () => {
    setMessage(null);
    setNeedsAuth(false);
    gitProgress.reset();
    setCloning(true);
    const run = ++runRef.current;
    const cloneId = crypto.randomUUID();
    cloneIdRef.current = cloneId;
    try {
      setMessage("Клонирование...");
      const project = await gitClone(url.trim(), destination.trim(), cloneId);
      if (runRef.current !== run) return;
      setCloning(false);
      onOpened?.(project);
    } catch (e) {
      if (runRef.current !== run) return;
      setCloning(false);
      const msg = toMessage(e);
      if (msg.startsWith("no_ssh_credentials:")) {
        setNeedsAuth(true);
        setMessage(
          "Аутентификация не настроена. Добавьте SSH ключ в настройках, чтобы продолжить.",
        );
      } else {
        setMessage(msg);
      }
    }
  }, [destination, gitProgress, onOpened, url]);

  /** Give the user back control immediately, then ask the backend to stop.
   * Deliberately not awaited: the whole point is that the UI must not depend
   * on a clone that may never respond. */
  const cancel = useCallback(() => {
    runRef.current += 1;
    const cloneId = cloneIdRef.current;
    cloneIdRef.current = null;
    setCloning(false);
    setMessage(null);
    gitProgress.reset();
    if (cloneId) void gitCloneCancel(cloneId).catch(() => {});
  }, [gitProgress]);

  /** The original called this `canSubmit` while passing it straight to
   * `disabled` — it is the negation. Named for what it does. */
  const submitDisabled =
    !url.trim() || !baseDir.trim() || !repoName || cloning || conflict;

  return {
    url,
    setUrl,
    destination,
    setDestination,
    pickDestination,
    message,
    cloning,
    busyLabel,
    needsAuth,
    conflict,
    stalled,
    submit,
    cancel,
    submitDisabled,
  };
}
