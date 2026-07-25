import { FileText, Folder, FolderOpen } from "lucide-react";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import type { TreeNode } from "../../lib/project";
import "./FileTree.css";

type FileTreeProps = {
  nodes: TreeNode[];
  rootName: string;
  rootPath: string;
  activePath: string | null;
  onOpenFile: (path: string) => void;
  onNewFile: (parentPath: string) => void;
  onNewFolder: (parentPath: string) => void;
};

type FileTreeNodeProps = {
  node: TreeNode;
  depth: number;
  activePath: string | null;
  onOpenFile: (path: string) => void;
  onContextMenu: (
    event: ReactMouseEvent,
    parentPath: string,
  ) => void;
};

type ContextMenuState = {
  x: number;
  y: number;
  parentPath: string;
};

const MENU_WIDTH = 200;
const MENU_HEIGHT = 72;

function parentOfFile(path: string): string {
  const parts = path.split(/[/\\]/);
  if (parts.length <= 1) return ".";
  return parts.slice(0, -1).join("/");
}

function FileTreeNode({
  node,
  depth,
  activePath,
  onOpenFile,
  onContextMenu,
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
          onContextMenu={(event) => onContextMenu(event, node.path)}
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
                onContextMenu={onContextMenu}
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
      onContextMenu={(event) => onContextMenu(event, parentOfFile(node.path))}
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
  onNewFile,
  onNewFolder,
}: FileTreeProps) {
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!menu || !menuRef.current) return;
    const rect = menuRef.current.getBoundingClientRect();
    const maxX = window.innerWidth - rect.width - 4;
    const maxY = window.innerHeight - rect.height - 4;
    const x = Math.max(4, Math.min(menu.x, maxX));
    const y = Math.max(4, Math.min(menu.y, maxY));
    if (x !== menu.x || y !== menu.y) {
      setMenu({ ...menu, x, y });
    }
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) {
        setMenu(null);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setMenu(null);
    };
    const onScroll = () => setMenu(null);
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, [menu]);

  const openContextMenu = (
    event: ReactMouseEvent,
    parentPath: string,
  ) => {
    event.preventDefault();
    event.stopPropagation();
    window.getSelection()?.removeAllRanges();
    const x = Math.min(event.clientX, window.innerWidth - MENU_WIDTH - 4);
    const y = Math.min(event.clientY, window.innerHeight - MENU_HEIGHT - 4);
    setMenu({ x: Math.max(4, x), y: Math.max(4, y), parentPath });
  };

  return (
    <div className="file-tree">
      <div className="file-tree-branch">
        <div
          className="file-tree-row dir root"
          style={{ paddingLeft: 4 }}
          title={rootPath}
          onContextMenu={(event) => openContextMenu(event, ".")}
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
              onContextMenu={openContextMenu}
            />
          ))
        )}
      </div>

      {menu ? (
        <div
          ref={menuRef}
          className="file-tree-context-menu"
          role="menu"
          style={{ left: menu.x, top: menu.y }}
        >
          <button
            type="button"
            role="menuitem"
            className="file-tree-context-item"
            onClick={() => {
              onNewFile(menu.parentPath);
              setMenu(null);
            }}
          >
            Новый файл…
          </button>
          <button
            type="button"
            role="menuitem"
            className="file-tree-context-item"
            onClick={() => {
              onNewFolder(menu.parentPath);
              setMenu(null);
            }}
          >
            Новая папка…
          </button>
        </div>
      ) : null}
    </div>
  );
}
