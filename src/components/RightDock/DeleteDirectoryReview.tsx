import { useEffect, useState } from "react";
import { listDocsTree, type TreeNode } from "../../lib/project";

type DeleteDirectoryReviewProps = {
  docsRoot: string;
  path: string;
};

const MAX_LISTED_ENTRIES = 20;

function findNode(nodes: TreeNode[], path: string): TreeNode | null {
  for (const node of nodes) {
    if (node.path === path) return node;
    if (node.children) {
      const found = findNode(node.children, path);
      if (found) return found;
    }
  }
  return null;
}

/** Depth-first flatten of `node`'s subtree into relative paths, directories
 * first at each level (matches how the sidebar tree itself sorts) — capped
 * by the caller at `MAX_LISTED_ENTRIES`, since a large directory would
 * otherwise render an unbounded list. */
function flatten(node: TreeNode): TreeNode[] {
  if (!node.children) return [];
  const out: TreeNode[] = [];
  for (const child of node.children) {
    out.push(child);
    if (child.children) out.push(...flatten(child));
  }
  return out;
}

/** Read-only preview of a pending `deleteDirectory` call — lists what's
 * actually inside the target directory (no dedicated "list one subdirectory"
 * command exists, so this fetches the full docs tree via `listDocsTree` and
 * filters client-side to the target's subtree), so the user isn't approving
 * a bare path with no sense of what it actually contains. */
export function DeleteDirectoryReview({ docsRoot, path }: DeleteDirectoryReviewProps) {
  const [entries, setEntries] = useState<TreeNode[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(false);
    listDocsTree(docsRoot)
      .then((tree) => {
        if (cancelled) return;
        const node = findNode(tree, path);
        setEntries(node ? flatten(node) : []);
      })
      .catch(() => {
        if (!cancelled) setError(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [docsRoot, path]);

  if (loading) {
    return <div className="tool-approval-diff-placeholder">Загрузка содержимого папки…</div>;
  }
  if (error) {
    return (
      <div className="tool-approval-diff-placeholder tool-approval-diff-error">
        Не удалось загрузить содержимое папки
      </div>
    );
  }
  if (!entries || entries.length === 0) {
    return <div className="tool-approval-diff-placeholder">Папка пуста</div>;
  }

  const shown = entries.slice(0, MAX_LISTED_ENTRIES);
  const hiddenCount = entries.length - shown.length;

  return (
    <ul className="tool-approval-dir-list">
      {shown.map((entry) => (
        <li key={entry.path} className="tool-approval-dir-list-item">
          {entry.isDir ? `${entry.path}/` : entry.path}
        </li>
      ))}
      {hiddenCount > 0 ? (
        <li className="tool-approval-dir-list-more">и ещё {hiddenCount}…</li>
      ) : null}
    </ul>
  );
}
