import { useEffect } from "react";
import { setEmbeddingPriorityFiles } from "../lib/embeddings";

type UseEmbeddingPriorityFilesOptions = {
  active?: boolean;
};

/** Fire-and-forget: tells the backend which files are open in the editor,
 * so a fresh project's first `embedding_sync` can chunk+embed those (and
 * their direct includes/xrefs) before the rest of the repo — see
 * `embedding_sync`'s first-sync branch and `PriorityFilesSlot`.
 *
 * Depends on a stable, sorted, newline-joined key derived from
 * `openTabPaths` rather than the array itself — `EditorTab.content`/`dirty`
 * changing on every keystroke would otherwise create a new array reference
 * (same open files, different identity) and re-fire this on every edit. */
export function useEmbeddingPriorityFiles(
  openTabPaths: string[],
  options: UseEmbeddingPriorityFilesOptions = {},
) {
  const { active = true } = options;
  const key = [...openTabPaths].sort().join("\n");

  useEffect(() => {
    if (!active) return;
    const paths = key === "" ? [] : key.split("\n");
    void setEmbeddingPriorityFiles(paths).catch(() => {
      // Best-effort hint — never load-bearing for correctness, so nothing
      // needs to surface this to the user.
    });
    // `key` already captures every dependency that matters here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, active]);
}
