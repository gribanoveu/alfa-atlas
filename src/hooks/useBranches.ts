import { useCallback, useEffect, useState } from "react";
import {
  gitCheckoutBranch,
  gitCreateBranch,
  gitListBranches,
  type GitBranchInfo,
} from "../lib/git";

type UseBranchesOptions = {
  active?: boolean;
};

export function useBranches(
  repoRoot: string | null,
  options: UseBranchesOptions = {},
) {
  const { active = true } = options;
  const [branches, setBranches] = useState<GitBranchInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!repoRoot) {
      setBranches([]);
      return;
    }
    try {
      const nextBranches = await gitListBranches(repoRoot);
      setBranches(nextBranches);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [repoRoot]);

  useEffect(() => {
    if (!active || !repoRoot) {
      if (!repoRoot) {
        setBranches([]);
      }
      return;
    }
    void refresh();
  }, [active, repoRoot, refresh]);

  const run = useCallback(
    async (op: () => Promise<void>): Promise<boolean> => {
      if (!repoRoot) return false;
      setBusy(true);
      try {
        await op();
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

  const createBranch = useCallback(
    (name: string, discardChanges = false) => {
      if (!repoRoot) return Promise.resolve(false);
      return run(() => gitCreateBranch(repoRoot, name, discardChanges));
    },
    [repoRoot, run],
  );

  const checkoutBranch = useCallback(
    (name: string, discardChanges = false) => {
      if (!repoRoot) return Promise.resolve(false);
      return run(() => gitCheckoutBranch(repoRoot, name, discardChanges));
    },
    [repoRoot, run],
  );

  return {
    branches,
    busy,
    error,
    refresh,
    createBranch,
    checkoutBranch,
  };
}
