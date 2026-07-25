import { FileText, Folder, FolderOpen } from "lucide-react";
import { useState } from "react";
import type { TreeNode } from "../../lib/project";
import "./FileTree.css";

type FileTreeProps = {
  nodes: TreeNode[];
  rootName: string;
  rootPath: string;
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
  const [expanded, setExpanded] = useState(depth < 3);

  if (node.isDir) {
    return (
      <div className="file-tree-branch">
        <button
          type="button"
          className="file-tree-row dir"
          style={{ paddingLeft: 4 + depth * 14 }}
          onClick={() => setExpanded((v) => !v)}
        >
          <span className="file-tree-twist">{expanded ? "▾" : "▸"}</span>
          {expanded ? (
            <FolderOpen className="file-tree-icon folder" size={14} aria-hidden />
          ) : (
            <Folder className="file-tree-icon folder" size={14} aria-hidden />
          )}
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
      style={{ paddingLeft: 4 + depth * 14 + 14 }}
      onClick={() => onOpenFile(node.path)}
      title={node.path}
    >
      <span className="file-tree-twist" />
      <FileText className="file-tree-icon file" size={14} aria-hidden />
      <span className="file-tree-name">{node.name}</span>
    </button>
  );
}

export function FileTree({
  nodes,
  rootName,
  rootPath,
  activePath,
  onOpenFile,
}: FileTreeProps) {
  return (
    <div className="file-tree">
      <div className="file-tree-branch">
        <div
          className="file-tree-row dir root"
          style={{ paddingLeft: 4 }}
          title={rootPath}
        >
          <span className="file-tree-twist">▾</span>
          <FolderOpen className="file-tree-icon folder" size={14} aria-hidden />
          <span className="file-tree-name">{rootName}</span>
        </div>
        {nodes.length === 0 ? (
          <div className="file-tree-empty">Нет поддерживаемых файлов</div>
        ) : (
          nodes.map((node) => (
            <FileTreeNode
              key={node.path}
              node={node}
              depth={1}
              activePath={activePath}
              onOpenFile={onOpenFile}
            />
          ))
        )}
      </div>
    </div>
  );
}
