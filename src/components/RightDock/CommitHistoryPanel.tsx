import { Folder, FolderOpen, RefreshCw } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { GitCommitSummary, GitFileStatus } from "../../lib/git";
import "./GitPanel.css";
import "./CommitHistoryPanel.css";

type CommitHistoryPanelProps = {
  commits: GitCommitSummary[];
  busy: boolean;
  error: string | null;
  onRefresh: () => void;
  onLoadCommitFiles: (commitHash: string) => Promise<GitFileStatus[] | null>;
  onOpenCommitFileDiff: (commitHash: string, file: GitFileStatus) => void;
};

const STATUS_LABELS: Record<string, string> = {
  M: "Изменён",
  A: "Добавлен",
  D: "Удалён",
  R: "Переименован",
};

function statusLabel(status: string): string {
  return STATUS_LABELS[status] ?? status;
}

function formatCommitTime(unixSeconds: number): string {
  try {
    return new Date(unixSeconds * 1000).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return "";
  }
}

type FileTreeDir = {
  kind: "dir";
  name: string;
  path: string;
  children: FileTreeNode[];
};

type FileTreeFile = {
  kind: "file";
  name: string;
  path: string;
  status: string;
};

type FileTreeNode = FileTreeDir | FileTreeFile;

function buildFileTree(files: GitFileStatus[]): FileTreeNode[] {
  const root: FileTreeDir = { kind: "dir", name: "", path: "", children: [] };

  for (const file of files) {
    const parts = file.path.replace(/\\/g, "/").split("/").filter(Boolean);
    let cursor = root;
    for (let i = 0; i < parts.length - 1; i++) {
      const name = parts[i];
      const path = cursor.path ? `${cursor.path}/${name}` : name;
      let next = cursor.children.find(
        (c): c is FileTreeDir => c.kind === "dir" && c.name === name,
      );
      if (!next) {
        next = { kind: "dir", name, path, children: [] };
        cursor.children.push(next);
      }
      cursor = next;
    }
    const name = parts[parts.length - 1] ?? file.path;
    cursor.children.push({
      kind: "file",
      name,
      path: file.path,
      status: file.status,
    });
  }

  sortTree(root.children);
  return root.children;
}

function sortTree(nodes: FileTreeNode[]): void {
  nodes.sort((a, b) => {
    if (a.kind !== b.kind) return a.kind === "dir" ? -1 : 1;
    return a.name.localeCompare(b.name);
  });
  for (const node of nodes) {
    if (node.kind === "dir") sortTree(node.children);
  }
}

type CommitFileTreeNodeProps = {
  node: FileTreeNode;
  depth: number;
  collapsedDirs: ReadonlySet<string>;
  onToggleDir: (path: string) => void;
  onOpenFile: (file: GitFileStatus) => void;
};

function CommitFileTreeNode({
  node,
  depth,
  collapsedDirs,
  onToggleDir,
  onOpenFile,
}: CommitFileTreeNodeProps) {
  if (node.kind === "dir") {
    const collapsed = collapsedDirs.has(node.path);
    return (
      <div className="commit-tree-branch">
        <button
          type="button"
          className="commit-tree-row dir"
          style={{ paddingLeft: 4 + depth * 14 }}
          onClick={() => onToggleDir(node.path)}
          aria-expanded={!collapsed}
        >
          <span className="commit-tree-twist">{collapsed ? "▸" : "▾"}</span>
          {collapsed ? (
            <Folder className="commit-tree-icon" size={14} aria-hidden />
          ) : (
            <FolderOpen className="commit-tree-icon" size={14} aria-hidden />
          )}
          <span className="commit-tree-name">{node.name}</span>
        </button>
        {!collapsed
          ? node.children.map((child) => (
              <CommitFileTreeNode
                key={child.path}
                node={child}
                depth={depth + 1}
                collapsedDirs={collapsedDirs}
                onToggleDir={onToggleDir}
                onOpenFile={onOpenFile}
              />
            ))
          : null}
      </div>
    );
  }

  const title = statusLabel(node.status);
  return (
    <button
      type="button"
      className="commit-tree-row file"
      style={{ paddingLeft: 4 + depth * 14 }}
      title={node.path}
      onClick={() => onOpenFile({ path: node.path, status: node.status })}
      aria-label={`Показать diff: ${node.path}`}
    >
      <span className="commit-tree-twist" />
      <span className="commit-tree-name">{node.name}</span>
      <span
        className={`git-status git-status-${node.status} commit-tree-status`}
        title={title}
        aria-label={title}
      >
        {node.status}
      </span>
    </button>
  );
}

export function CommitHistoryPanel({
  commits,
  busy,
  error,
  onRefresh,
  onLoadCommitFiles,
  onOpenCommitFileDiff,
}: CommitHistoryPanelProps) {
  const [selectedHash, setSelectedHash] = useState<string | null>(null);
  const [files, setFiles] = useState<GitFileStatus[]>([]);
  const [filesLoading, setFilesLoading] = useState(false);
  const [filesError, setFilesError] = useState<string | null>(null);
  const [collapsedDirs, setCollapsedDirs] = useState<ReadonlySet<string>>(
    new Set(),
  );

  useEffect(() => {
    if (!selectedHash) {
      setFiles([]);
      setFilesError(null);
      return;
    }
    let cancelled = false;
    setFilesLoading(true);
    setFilesError(null);
    void onLoadCommitFiles(selectedHash).then((result) => {
      if (cancelled) return;
      if (!result) setFilesError("Не удалось загрузить список файлов");
      else setFiles(result);
      // Start every commit's tree fully expanded.
      setCollapsedDirs(new Set());
      setFilesLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, [selectedHash, onLoadCommitFiles]);

  const tree = useMemo(() => buildFileTree(files), [files]);

  const toggleDir = (path: string) => {
    setCollapsedDirs((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  return (
    <div className="commit-history-panel">
      <div className="commit-history-list git-panel">
        <div className="git-panel-toolbar">
          <button
            type="button"
            className="git-icon-btn"
            title="Обновить список"
            aria-label="Обновить список"
            disabled={busy}
            onClick={onRefresh}
          >
            <RefreshCw size={14} aria-hidden />
          </button>
        </div>

        <div className="git-panel-scroll">
          {commits.length === 0 ? (
            <div className="git-empty" style={{ paddingLeft: 8 }}>
              Записей в истории пока нет
            </div>
          ) : (
            <ul className="git-commit-list">
              {commits.map((item) => (
                <li
                  key={item.hash + String(item.time)}
                  className={`git-commit-row git-commit-row-flat${
                    selectedHash === item.hash ? " git-commit-row-selected" : ""
                  }`}
                >
                  <button
                    type="button"
                    className="git-commit-row-btn"
                    onClick={() => setSelectedHash(item.hash)}
                    aria-pressed={selectedHash === item.hash}
                  >
                    <div className="git-commit-line">
                      <span className="git-commit-hash">{item.hash}</span>
                      <span className="git-commit-msg">{item.message}</span>
                    </div>
                    <div className="git-commit-meta">
                      {item.author}
                      {item.author ? " · " : null}
                      {formatCommitTime(item.time)}
                    </div>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>

        {error ? (
          <div className="git-panel-error git-panel-error-dock">{error}</div>
        ) : null}
      </div>

      <div className="commit-history-files git-panel">
        <div className="git-panel-scroll">
          {!selectedHash ? (
            <div className="git-empty" style={{ paddingLeft: 8 }}>
              Выберите коммит, чтобы посмотреть изменённые файлы
            </div>
          ) : filesLoading ? (
            <div className="git-empty" style={{ paddingLeft: 8 }}>
              Загрузка файлов…
            </div>
          ) : filesError ? (
            <div className="git-panel-error" style={{ padding: "6px 8px" }}>
              {filesError}
            </div>
          ) : files.length === 0 ? (
            <div className="git-empty" style={{ paddingLeft: 8 }}>
              Нет изменённых файлов
            </div>
          ) : (
            tree.map((node) => (
              <CommitFileTreeNode
                key={node.path}
                node={node}
                depth={0}
                collapsedDirs={collapsedDirs}
                onToggleDir={toggleDir}
                onOpenFile={(file) => onOpenCommitFileDiff(selectedHash, file)}
              />
            ))
          )}
        </div>
      </div>
    </div>
  );
}
