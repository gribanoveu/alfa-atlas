import { useCallback, useEffect, useState } from "react";
import { listDocsTree, type TreeNode } from "../lib/project";

export function useDocsTree(docsRoot: string | null) {
  const [nodes, setNodes] = useState<TreeNode[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!docsRoot) {
      setNodes([]);
      setError(null);
      return;
    }
    setLoading(true);
    try {
      const tree = await listDocsTree(docsRoot);
      setNodes(tree);
      setError(null);
    } catch (e) {
      setNodes([]);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [docsRoot]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { nodes, loading, error, refresh };
}
