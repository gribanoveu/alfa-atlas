import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { INDEX_EVENT_CHANNEL, type IndexEvent } from "../lib/workspaceIndex";
import {
  gitAbortMerge,
  gitApplyDiffContent,
  gitCommit,
  gitConflictFileContent,
  gitDiscardFileChanges,
  gitFileDiff,
  gitFinishMerge,
  gitLog,
  gitPull,
  gitPush,
  gitResetToRemote,
  gitResolveConflict,
  gitStage,
  gitStatus,
  gitUnstage,
  type GitCommitSummary,
  type GitConflictFile,
  type GitDiffScope,
  type GitFileDiff,
  type GitStatusSnapshot,
  type PullMode,
} from "../lib/git";

const EMPTY_STATUS: GitStatusSnapshot = {
  staged: [],
  unstaged: [],
  conflicted: [],
  branch: null,
  hasCommits: false,
  hasUpstream: false,
  ahead: 0,
  mergeInProgress: false,
};

type UseGitPanelOptions = {
  active?: boolean;
  onBranchChange?: (branch: string | null) => void;
};

/** Outcome of pull(): "ok" (merged cleanly), "conflict" (surfaced via
 * status.conflicted instead of an error — no separate message here), or a
 * real failure (network/auth/no-upstream/etc). */
export type PullResult =
  | { status: "ok" }
  | { status: "conflict" }
  | { status: "error"; message: string };

/** Formats commit message as `doc(JIRA-123): description` or `doc(): description`. */
export function formatDocCommitMessage(
  jiraKey: string,
  description: string,
): string | null {
  const desc = description.trim();
  if (!desc) return null;
  return `doc(${jiraKey.trim()}): ${desc}`;
}

export function useGitPanel(
  repoRoot: string | null,
  options: UseGitPanelOptions = {},
) {
  const { active = true, onBranchChange } = options;
  const [status, setStatus] = useState<GitStatusSnapshot>(EMPTY_STATUS);
  const [commits, setCommits] = useState<GitCommitSummary[]>([]);
  const [jiraKey, setJiraKey] = useState("");
  const [description, setDescription] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const onBranchChangeRef = useRef(onBranchChange);
  onBranchChangeRef.current = onBranchChange;
  const refreshTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const refresh = useCallback(async () => {
    if (!repoRoot) {
      setStatus(EMPTY_STATUS);
      setCommits([]);
      onBranchChangeRef.current?.(null);
      return;
    }
    try {
      const nextStatus = await gitStatus(repoRoot);
      setStatus(nextStatus);
      onBranchChangeRef.current?.(nextStatus.branch);
      try {
        const nextCommits = await gitLog(repoRoot, 20);
        setCommits(nextCommits);
        setError(null);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [repoRoot]);

  const scheduleRefresh = useCallback(() => {
    if (refreshTimer.current) clearTimeout(refreshTimer.current);
    refreshTimer.current = setTimeout(() => {
      void refresh();
    }, 250);
  }, [refresh]);

  useEffect(() => {
    if (!active || !repoRoot) {
      if (!repoRoot) {
        setStatus(EMPTY_STATUS);
        setCommits([]);
      }
      return;
    }
    void refresh();
  }, [active, repoRoot, refresh]);

  useEffect(() => {
    return () => {
      if (refreshTimer.current) clearTimeout(refreshTimer.current);
    };
  }, []);

  // The workspace file watcher (services/file_watcher.rs) already emits
  // indexUpdated whenever a tracked document changes on disk — including
  // edits made outside the app (another editor, a `git checkout` run in a
  // terminal). Piggyback on that existing channel instead of adding a
  // second watcher: it's the same "did a tracked file change" signal the
  // git panel needs, just not wired to it previously. Known limitation:
  // indexUpdated only fires for supported document extensions (see
  // is_supported_file in file_watcher.rs), so external edits to other
  // tracked files (e.g. .gitignore) won't trigger this.
  useEffect(() => {
    if (!active || !repoRoot) return;
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    listen<IndexEvent>(INDEX_EVENT_CHANNEL, (event) => {
      if (cancelled) return;
      if (event.payload.kind === "indexUpdated") {
        scheduleRefresh();
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, [active, repoRoot, scheduleRefresh]);

  const stage = useCallback(
    async (paths: string[]) => {
      if (!repoRoot || paths.length === 0) return;
      setBusy(true);
      try {
        await gitStage(repoRoot, paths);
        await refresh();
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
    },
    [refresh, repoRoot],
  );

  const unstage = useCallback(
    async (paths: string[]) => {
      if (!repoRoot || paths.length === 0) return;
      setBusy(true);
      try {
        await gitUnstage(repoRoot, paths);
        await refresh();
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
    },
    [refresh, repoRoot],
  );

  /** Returns the new commit's short hash, or null if nothing was committed
   * (no repo, no message, nothing staged) or the commit failed. */
  const commit = useCallback(async (): Promise<string | null> => {
    if (!repoRoot) return null;
    const formatted = formatDocCommitMessage(jiraKey, description);
    if (!formatted || status.staged.length === 0) return null;
    setBusy(true);
    try {
      const hash = await gitCommit(repoRoot, formatted);
      setJiraKey("");
      setDescription("");
      await refresh();
      return hash;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return null;
    } finally {
      setBusy(false);
    }
  }, [description, jiraKey, refresh, repoRoot, status.staged.length]);

  const runRemote = useCallback(
    async (op: () => Promise<void>): Promise<string | null> => {
      if (!repoRoot) return "Нет открытого репозитория";
      setBusy(true);
      try {
        await op();
        await refresh();
        return null;
      } catch (e) {
        return e instanceof Error ? e.message : String(e);
      } finally {
        setBusy(false);
      }
    },
    [refresh, repoRoot],
  );

  const pull = useCallback(
    async (mode: PullMode): Promise<PullResult> => {
      if (!repoRoot) return { status: "error", message: "Нет открытого репозитория" };
      setBusy(true);
      try {
        await gitPull(repoRoot, mode);
        await refresh();
        return { status: "ok" };
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        // A merge conflict leaves the repo mid-merge instead of raising a
        // hard error — surface it through `status.conflicted` (picked up by
        // the fresh status below) rather than a generic error alert.
        try {
          const fresh = await gitStatus(repoRoot);
          setStatus(fresh);
          onBranchChangeRef.current?.(fresh.branch);
          if (fresh.conflicted.length > 0) {
            setError(null);
            return { status: "conflict" };
          }
        } catch {
          // Ignore — fall through and report the original pull error.
        }
        setError(message);
        return { status: "error", message };
      } finally {
        setBusy(false);
      }
    },
    [refresh, repoRoot],
  );

  const loadConflictFile = useCallback(
    async (path: string): Promise<GitConflictFile | null> => {
      if (!repoRoot) return null;
      try {
        const file = await gitConflictFileContent(repoRoot, path);
        setError(null);
        return file;
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        return null;
      }
    },
    [repoRoot],
  );

  const resolveConflict = useCallback(
    async (
      path: string,
      content: string,
    ): Promise<{ ok: boolean; mergeFinished: boolean; commitHash?: string }> => {
      if (!repoRoot) return { ok: false, mergeFinished: false };
      setBusy(true);
      try {
        await gitResolveConflict(repoRoot, path, content);
        const fresh = await gitStatus(repoRoot);
        setStatus(fresh);
        onBranchChangeRef.current?.(fresh.branch);
        if (fresh.conflicted.length === 0 && fresh.mergeInProgress) {
          const commitHash = await gitFinishMerge(repoRoot);
          await refresh();
          setError(null);
          return { ok: true, mergeFinished: true, commitHash };
        }
        setError(null);
        return { ok: true, mergeFinished: false };
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        return { ok: false, mergeFinished: false };
      } finally {
        setBusy(false);
      }
    },
    [refresh, repoRoot],
  );

  const abortMerge = useCallback(async (): Promise<boolean> => {
    if (!repoRoot) return false;
    setBusy(true);
    try {
      await gitAbortMerge(repoRoot);
      await refresh();
      setError(null);
      return true;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return false;
    } finally {
      setBusy(false);
    }
  }, [refresh, repoRoot]);

  /** Manual retry for the rare case where the last resolveConflict() call
   * cleared all conflicts but the automatic finish-merge commit failed
   * (e.g. missing git identity) — surfaced as a banner in the git panel. */
  const finishMerge = useCallback(async (): Promise<boolean> => {
    if (!repoRoot) return false;
    setBusy(true);
    try {
      await gitFinishMerge(repoRoot);
      await refresh();
      setError(null);
      return true;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return false;
    } finally {
      setBusy(false);
    }
  }, [refresh, repoRoot]);

  const resetToRemote = useCallback(() => {
    if (!repoRoot) return Promise.resolve("Нет открытого репозитория");
    return runRemote(() => gitResetToRemote(repoRoot));
  }, [repoRoot, runRemote]);

  const push = useCallback(() => {
    if (!repoRoot) return Promise.resolve("Нет открытого репозитория");
    return runRemote(() => gitPush(repoRoot));
  }, [repoRoot, runRemote]);

  const loadFileDiff = useCallback(
    async (path: string, scope: GitDiffScope): Promise<GitFileDiff | null> => {
      if (!repoRoot) return null;
      try {
        const diff = await gitFileDiff(repoRoot, path, scope);
        setError(null);
        return diff;
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        return null;
      }
    },
    [repoRoot],
  );

  const discardFileChanges = useCallback(
    async (path: string): Promise<{ ok: true; backupId: string | null } | { ok: false }> => {
      if (!repoRoot) return { ok: false };
      setBusy(true);
      try {
        const backupId = await gitDiscardFileChanges(repoRoot, path);
        await refresh();
        setError(null);
        return { ok: true, backupId };
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        return { ok: false };
      } finally {
        setBusy(false);
      }
    },
    [refresh, repoRoot],
  );

  const applyDiffContent = useCallback(
    async (path: string, scope: GitDiffScope, content: string): Promise<boolean> => {
      if (!repoRoot) return false;
      setBusy(true);
      try {
        await gitApplyDiffContent(repoRoot, path, scope, content);
        await refresh();
        setError(null);
        return true;
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        return false;
      } finally {
        setBusy(false);
      }
    },
    [refresh, repoRoot],
  );

  const canCommit =
    status.staged.length > 0 && description.trim().length > 0 && !busy;

  return {
    status,
    commits,
    jiraKey,
    setJiraKey,
    description,
    setDescription,
    error,
    busy,
    canCommit,
    refresh,
    scheduleRefresh,
    stage,
    unstage,
    commit,
    pull,
    resetToRemote,
    push,
    loadFileDiff,
    discardFileChanges,
    applyDiffContent,
    loadConflictFile,
    resolveConflict,
    abortMerge,
    finishMerge,
  };
}
