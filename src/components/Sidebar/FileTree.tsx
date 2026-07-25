import { useState } from "react";
import type { TreeNode } from "../../lib/project";
import "./FileTree.css";

type FileTreeProps = {
  nodes: TreeNode[];
  activePath: string | null;
  onOpenFile: (path: string) => void;
};

type FileTreeNodeProps = {
  node: TreeNode;
  depth: number;
  activePath: string | null;
  onOpenFile: (path: string) => void;
};

function FileTreeNode({
  node,
  depth,
  activePath,
  onOpenFile,
}: FileTreeNodeProps) {
  const [expanded, setExpanded] = useState(depth < 2);

  if (node.isDir) {
    return (
      <div className="file-tree-branch">
        <button
          type="button"
          className="file-tree-row dir"
          style={{ paddingLeft: 8 + depth * 12 }}
          onClick={() => setExpanded((v) => !v)}
        >
          <span className="file-tree-twist">{expanded ? "▾" : "▸"}</span>
          <span className="file-tree-name">{node.name}</span>
        </button>
        {expanded && node.children
          ? node.children.map((child) => (
              <FileTreeNode
                key={child.path}
                node={child}
                depth={depth + 1}
                activePath={activePath}
                onOpenFile={onOpenFile}
              />
            ))
          : null}
      </div>
    );
  }

  const active = activePath === node.path;
  return (
    <button
      type="button"
      className={`file-tree-row file${active ? " active" : ""}`}
      style={{ paddingLeft: 8 + depth * 12 + 14 }}
      onClick={() => onOpenFile(node.path)}
      title={node.path}
    >
      <span className="file-tree-name">{node.name}</span>
    </button>
  );
}

export function FileTree({ nodes, activePath, onOpenFile }: FileTreeProps) {
  if (nodes.length === 0) {
    return <div className="panel-empty">Нет поддерживаемых файлов</div>;
  }

  return (
    <div className="file-tree">
      {nodes.map((node) => (
        <FileTreeNode
          key={node.path}
          node={node}
          depth={0}
          activePath={activePath}
          onOpenFile={onOpenFile}
        />
      ))}
    </div>
  );
}
