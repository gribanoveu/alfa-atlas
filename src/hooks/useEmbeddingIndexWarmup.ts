import { useEffect } from "react";
import { getEmbeddingIndexStatus, teardownIncrementalWatcher } from "../lib/embeddings";

type UseEmbeddingIndexWarmupOptions = {
  active?: boolean;
};

/** Fire-and-forget: calls `embedding_index_status` as soon as a project
 * opens, purely so the backend attaches (and, on a cold start, reloads
 * from disk) `ChunkIndex`/`EmbeddingIndex` right away instead of waiting
 * for the user to open the Assistant panel or the Settings "Эмбеддинги"
 * tab — this is also what starts the file-watcher-driven incremental sync
 * on the backend (`ensure_incremental_watcher`), so edits start flowing
 * into the index immediately rather than only after a manual sync. Mirrors
 * `useWorkspaceIndex`'s "trigger on repoRoot" pattern, but deliberately
 * doesn't return or store any state — `useEmbeddingSetup`'s own fetch (in
 * whichever panel the user eventually opens) is what the UI actually
 * reads; this just makes that later fetch fast and accurate by warming the
 * backend early.
 *
 * Tears the watcher down on cleanup (project closed / this component
 * unmounted) — the backend's own attach logic already swaps the watcher to
 * a *new* project's `index_root` on its own, but nothing else stops it for
 * "closed with no new project opened in this session", which would
 * otherwise leak a live `notify` watch pointed at a project that's no
 * longer open. */
export function useEmbeddingIndexWarmup(
  repoRoot: string | null,
  options: UseEmbeddingIndexWarmupOptions = {},
) {
  const { active = true } = options;

  useEffect(() => {
    if (!active || !repoRoot) return;
    void getEmbeddingIndexStatus().catch(() => {
      // Best-effort — a real fetch from useEmbeddingSetup surfaces actual
      // errors to the user once a panel that cares is opened.
    });
    return () => {
      void teardownIncrementalWatcher();
    };
  }, [active, repoRoot]);
}
