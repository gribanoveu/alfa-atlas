import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toMessage } from "../lib/errors";
import { gitClone, type ProbeResult } from "../lib/git";
import { checkPathExists } from "../lib/project";
import { getGeneralPrefs, setGeneralPrefs } from "../lib/prefs";
import { formatGitProgress, useGitProgress } from "./useGitProgress";

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

  const repoName = useMemo(() => getRepoName(url), [url]);
  const destination = repoName ? `${baseDir}/${repoName}` : baseDir;
  const progressLabel = cloning ? formatGitProgress(gitProgress.event) : null;

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
        if (!cancelled) setConflict(result.exists);
      });
    }, 400);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [destination]);

  const setDestination = useCallback((value: string) => {
    const lastSlash = value.lastIndexOf("/");
    setBaseDir(lastSlash > 0 ? value.slice(0, lastSlash) : value);
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
    try {
      setMessage("Клонирование...");
      const project = await gitClone(url.trim(), destination.trim());
      setCloning(false);
      onOpened?.(project);
    } catch (e) {
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
    progressLabel,
    needsAuth,
    conflict,
    submit,
    submitDisabled,
  };
}
