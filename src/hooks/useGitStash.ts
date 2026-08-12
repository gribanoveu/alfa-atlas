import { useCallback, useEffect, useState } from "react";
import {
  gitStashApply,
  gitStashDrop,
  gitStashList,
  type GitStashEntry,
  type GitStashRestoreOutcome,
} from "../lib/git";

type UseGitStashOptions = {
  active?: boolean;
};

/** The "Отложенные изменения" shelf — docflow-managed stash entries created
 * by auto-stashing uncommitted changes on branch switch. See useBranches'
 * checkoutBranch/checkoutRemoteBranch for where entries get created. */
export function useGitStash(
  repoRoot: string | null,
  options: UseGitStashOptions = {},
) {
  const { active = true } = options;
  const [entries, setEntries] = useState<GitStashEntry[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!repoRoot) {
      setEntries([]);
      return;
    }
    try {
      const next = await gitStashList(repoRoot);
      setEntries(next);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [repoRoot]);

  useEffect(() => {
    if (!active || !repoRoot) {
      if (!repoRoot) {
        setEntries([]);
      }
      return;
    }
    void refresh();
  }, [active, repoRoot, refresh]);

  const restore = useCallback(
    async (id: string): Promise<GitStashRestoreOutcome | null> => {
      if (!repoRoot) return null;
      setBusy(true);
      try {
        const outcome = await gitStashApply(repoRoot, id);
        await refresh();
        setError(null);
        return outcome;
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        return null;
      } finally {
        setBusy(false);
      }
    },
    [repoRoot, refresh],
  );

  const discard = useCallback(
    async (id: string): Promise<boolean> => {
      if (!repoRoot) return false;
      setBusy(true);
      try {
        await gitStashDrop(repoRoot, id);
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
    [repoRoot, refresh],
  );

  return {
    entries,
    busy,
    error,
    refresh,
    restore,
    discard,
  };
}
