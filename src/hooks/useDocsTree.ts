import { useCallback, useEffect, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import { listDocsTree, type TreeNode } from "../lib/project";

export function useDocsTree(docsRoot: string | null) {
  const [nodes, setNodes] = useState<TreeNode[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const hasLoadedRef = useRef(false);

  const refresh = useCallback(async () => {
    if (!docsRoot) {
      setNodes([]);
      setError(null);
      setLoading(false);
      hasLoadedRef.current = false;
      return;
    }

    const showLoading = !hasLoadedRef.current;
    if (showLoading) setLoading(true);

    try {
      const tree = await listDocsTree(docsRoot);
      setNodes(tree);
      setError(null);
      hasLoadedRef.current = true;
    } catch (e) {
      setNodes([]);
      setError(toMessage(e));
    } finally {
      if (showLoading) setLoading(false);
    }
  }, [docsRoot]);

  useEffect(() => {
    hasLoadedRef.current = false;
    void refresh();
  }, [refresh]);

  return { nodes, loading, error, refresh };
}
