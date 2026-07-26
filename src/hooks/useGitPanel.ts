import { useCallback, useEffect, useRef, useState } from "react";
import {
  gitCommit,
  gitDiscardFileChanges,
  gitFileDiff,
  gitLog,
  gitPull,
  gitPush,
  gitResetToRemote,
  gitStage,
  gitStatus,
  gitUnstage,
  type GitCommitSummary,
  type GitDiffScope,
  type GitFileDiff,
  type GitStatusSnapshot,
  type PullMode,
} from "../lib/git";

const EMPTY_STATUS: GitStatusSnapshot = {
  staged: [],
  unstaged: [],
  branch: null,
};

type UseGitPanelOptions = {
  active?: boolean;
  onBranchChange?: (branch: string | null) => void;
};

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
      return;
    }
    try {
      const [nextStatus, nextCommits] = await Promise.all([
        gitStatus(repoRoot),
        gitLog(repoRoot, 20),
      ]);
      setStatus(nextStatus);
      setCommits(nextCommits);
      setError(null);
      onBranchChangeRef.current?.(nextStatus.branch);
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

  const commit = useCallback(async () => {
    if (!repoRoot) return false;
    const formatted = formatDocCommitMessage(jiraKey, description);
    if (!formatted || status.staged.length === 0) return false;
    setBusy(true);
    try {
      await gitCommit(repoRoot, formatted);
      setJiraKey("");
      setDescription("");
      await refresh();
      return true;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return false;
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
    (mode: PullMode) => {
      if (!repoRoot) return Promise.resolve("Нет открытого репозитория");
      return runRemote(() => gitPull(repoRoot, mode));
    },
    [repoRoot, runRemote],
  );

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
    async (path: string): Promise<boolean> => {
      if (!repoRoot) return false;
      setBusy(true);
      try {
        await gitDiscardFileChanges(repoRoot, path);
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
  };
}
