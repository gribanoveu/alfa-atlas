import { useCallback, useEffect, useState } from "react";
import {
  gitActionLogAppend,
  gitActionLogList,
  gitActionLogMarkUndone,
  type GitActionLogEntry,
  type GitActionPayload,
} from "../lib/gitActionLog";

type UseGitActionLogOptions = {
  active?: boolean;
};

/** The persisted "what did I just do" log (SQLite-backed, see
 * infra::git_action_log_store) — hydrated per repo, updated optimistically
 * on record()/markUndone() so the UI never waits on the persistence
 * round-trip. Persistence failures are swallowed (best-effort, matching
 * this codebase's other non-critical local-store writes) since the
 * in-memory state already reflects the action either way. */
export function useGitActionLog(
  repoRoot: string | null,
  options: UseGitActionLogOptions = {},
) {
  const { active = true } = options;
  const [entries, setEntries] = useState<GitActionLogEntry[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!active || !repoRoot) {
      setEntries([]);
      return;
    }
    let cancelled = false;
    void gitActionLogList(repoRoot)
      .then((rows) => {
        if (!cancelled) setEntries(rows);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [active, repoRoot]);

  const record = useCallback(
    (entry: {
      kind: GitActionLogEntry["kind"];
      summary: string;
      undoable: boolean;
      payload: GitActionPayload;
    }) => {
      if (!repoRoot) return;
      const full: GitActionLogEntry = {
        ...entry,
        id: crypto.randomUUID(),
        createdAt: Date.now(),
        undone: false,
      };
      setEntries((prev) => [full, ...prev].slice(0, 50));
      void gitActionLogAppend(repoRoot, full).catch(() => {
        // Best-effort — the entry still lives in memory for this session.
      });
    },
    [repoRoot],
  );

  const markUndone = useCallback((id: string) => {
    setEntries((prev) => prev.map((e) => (e.id === id ? { ...e, undone: true } : e)));
    void gitActionLogMarkUndone(id).catch(() => {
      // Best-effort — see record()'s comment.
    });
  }, []);

  return { entries, error, record, markUndone };
}
