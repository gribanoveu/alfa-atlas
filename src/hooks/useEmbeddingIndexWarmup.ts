import { useEffect } from "react";
import { getEmbeddingIndexStatus } from "../lib/embeddings";

type UseEmbeddingIndexWarmupOptions = {
  active?: boolean;
};

/** Fire-and-forget: calls `embedding_index_status` as soon as a project
 * opens, purely so the backend attaches (and, on a cold start, reloads
 * from disk) `ChunkIndex`/`EmbeddingIndex` right away instead of waiting
 * for the user to open the Assistant panel or the Settings "Эмбеддинги"
 * tab. Mirrors `useWorkspaceIndex`'s "trigger on repoRoot" pattern, but
 * deliberately doesn't return or store any state — `useEmbeddingSetup`'s
 * own fetch (in whichever panel the user eventually opens) is what the UI
 * actually reads; this just makes that later fetch fast and accurate by
 * warming the backend early. */
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
  }, [active, repoRoot]);
}
