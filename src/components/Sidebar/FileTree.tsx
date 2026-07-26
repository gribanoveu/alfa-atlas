import {
  FilePlus,
  FileText,
  Folder,
  FolderOpen,
  FolderPlus,
  Pencil,
  Trash2,
} from "lucide-react";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import type { TreeNode } from "../../lib/project";
import { PanelResizeHandle } from "../PanelResizeHandle/PanelResizeHandle";
import "./FileTree.css";

const EXTERNAL_FOLDER_NAME = "_external";

export type FileTreeDeleteTarget = {
  path: string;
  isDir: boolean;
};

type FileTreeProps = {
  nodes: TreeNode[];
  rootName: string;
  rootPath: string;
  activePath: string | null;
  expandedDirs: ReadonlySet<string>;
  separateExternal?: boolean;
  onToggleDir: (path: string) => void;
  onOpenFile: (path: string) => void;
  onNewFile: (parentPath: string) => void;
  onNewFolder: (parentPath: string) => void;
  onRename: (target: FileTreeDeleteTarget) => void;
  onDelete: (target: FileTreeDeleteTarget) => void;
  onResizeExternal?: (delta: number) => void;
  onResizeExternalEnd?: () => void;
};

type FileTreeNodeProps = {
  node: TreeNode;
  depth: number;
  activePath: string | null;
  expandedDirs: ReadonlySet<string>;
  onToggleDir: (path: string) => void;
  onOpenFile: (path: string) => void;
  onContextMenu: (
    event: ReactMouseEvent,
    parentPath: string,
    target: FileTreeDeleteTarget | null,
  ) => void;
};

type ContextMenuState = {
  x: number;
  y: number;
  parentPath: string;
  target: FileTreeDeleteTarget | null;
};

const MENU_WIDTH = 200;
const MENU_HEIGHT = 148;

function parentOfFile(path: string): string {
  const parts = path.split(/[/\\]/);
  if (parts.length <= 1) return ".";
  return parts.slice(0, -1).join("/");
}

function FileTreeNode({
  node,
  depth,
  activePath,
  expandedDirs,
  onToggleDir,
  onOpenFile,
  onContextMenu,
}: FileTreeNodeProps) {
  if (node.isDir) {
    const expanded = expandedDirs.has(node.path);
    return (
      <div className="file-tree-branch">
        <button
          type="button"
          className="file-tree-row dir"
          style={{ paddingLeft: 4 + depth * 14 }}
          onClick={() => onToggleDir(node.path)}
          onContextMenu={(event) =>
            onContextMenu(event, node.path, {
              path: node.path,
              isDir: true,
            })
          }
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
                expandedDirs={expandedDirs}
                onToggleDir={onToggleDir}
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
      style={{ paddingLeft: 4 + depth * 14 }}
      onClick={() => onOpenFile(node.path)}
      onContextMenu={(event) =>
        onContextMenu(event, parentOfFile(node.path), {
          path: node.path,
          isDir: false,
        })
      }
      title={node.path}
    >
      <span className="file-tree-twist" />
      <FileText className="file-tree-icon file" size={14} aria-hidden />
      <span className="file-tree-name">{node.name}</span>
    </button>
  );
}

function splitExternalNodes(nodes: TreeNode[]): {
  main: TreeNode[];
  external: TreeNode | null;
} {
  const external =
    nodes.find((n) => n.isDir && n.name === EXTERNAL_FOLDER_NAME) ?? null;
  if (!external) return { main: nodes, external: null };
  return {
    main: nodes.filter((n) => n.path !== external.path),
    external,
  };
}

export function FileTree({
  nodes,
  rootName,
  rootPath,
  activePath,
  expandedDirs,
  separateExternal = true,
  onToggleDir,
  onOpenFile,
  onNewFile,
  onNewFolder,
  onRename,
  onDelete,
  onResizeExternal,
  onResizeExternalEnd,
}: FileTreeProps) {
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const rootExpanded = expandedDirs.has(".");
  const { main, external } = separateExternal
    ? splitExternalNodes(nodes)
    : { main: nodes, external: null };
  const externalExpanded = external
    ? expandedDirs.has(external.path)
    : false;
  const dockedExternal = Boolean(external);

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
    target: FileTreeDeleteTarget | null,
  ) => {
    event.preventDefault();
    event.stopPropagation();
    window.getSelection()?.removeAllRanges();
    const x = Math.min(event.clientX, window.innerWidth - MENU_WIDTH - 4);
    const y = Math.min(event.clientY, window.innerHeight - MENU_HEIGHT - 4);
    setMenu({ x: Math.max(4, x), y: Math.max(4, y), parentPath, target });
  };

  return (
    <div className={`file-tree${dockedExternal ? " has-external" : ""}`}>
      <div className="file-tree-main">
        <div className="file-tree-branch">
          <div
            className="file-tree-row dir root"
            style={{ paddingLeft: 4 }}
            title={rootPath}
            onClick={() => onToggleDir(".")}
            onContextMenu={(event) => openContextMenu(event, ".", null)}
          >
            <span className="file-tree-twist">{rootExpanded ? "▾" : "▸"}</span>
            {rootExpanded ? (
              <FolderOpen
                className="file-tree-icon folder"
                size={14}
                aria-hidden
              />
            ) : (
              <Folder className="file-tree-icon folder" size={14} aria-hidden />
            )}
            <span className="file-tree-name">{rootName}</span>
          </div>
          {rootExpanded ? (
            main.length === 0 && !external ? (
              <div className="file-tree-empty">Нет поддерживаемых файлов</div>
            ) : (
              main.map((node) => (
                <FileTreeNode
                  key={node.path}
                  node={node}
                  depth={1}
                  activePath={activePath}
                  expandedDirs={expandedDirs}
                  onToggleDir={onToggleDir}
                  onOpenFile={onOpenFile}
                  onContextMenu={openContextMenu}
                />
              ))
            )
          ) : null}
        </div>
      </div>

      {external ? (
        <div className="file-tree-external-dock">
          {onResizeExternal ? (
            <PanelResizeHandle
              direction="vertical"
              invert
              ariaLabel="Изменить высоту панели _external"
              onResize={onResizeExternal}
              onResizeEnd={onResizeExternalEnd}
            />
          ) : null}
          <div className="file-tree-external">
            <div className="file-tree-branch">
              <button
                type="button"
                className="file-tree-row dir external-root"
                style={{ paddingLeft: 4 }}
                title={external.path}
                onClick={() => onToggleDir(external.path)}
                onContextMenu={(event) =>
                  openContextMenu(event, external.path, {
                    path: external.path,
                    isDir: true,
                  })
                }
              >
                <span className="file-tree-twist">
                  {externalExpanded ? "▾" : "▸"}
                </span>
                {externalExpanded ? (
                  <FolderOpen
                    className="file-tree-icon folder external"
                    size={14}
                    aria-hidden
                  />
                ) : (
                  <Folder
                    className="file-tree-icon folder external"
                    size={14}
                    aria-hidden
                  />
                )}
                <span className="file-tree-name">{external.name}</span>
                <span className="file-tree-external-badge">external</span>
              </button>
              {externalExpanded ? (
                (external.children?.length ?? 0) === 0 ? (
                  <div className="file-tree-empty">Папка пуста</div>
                ) : (
                  external.children!.map((child) => (
                    <FileTreeNode
                      key={child.path}
                      node={child}
                      depth={1}
                      activePath={activePath}
                      expandedDirs={expandedDirs}
                      onToggleDir={onToggleDir}
                      onOpenFile={onOpenFile}
                      onContextMenu={openContextMenu}
                    />
                  ))
                )
              ) : null}
            </div>
          </div>
        </div>
      ) : null}

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
            <span className="file-tree-context-icon" aria-hidden>
              <FilePlus size={14} strokeWidth={1.75} />
            </span>
            <span className="file-tree-context-label">Новый файл…</span>
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
            <span className="file-tree-context-icon" aria-hidden>
              <FolderPlus size={14} strokeWidth={1.75} />
            </span>
            <span className="file-tree-context-label">Новая папка…</span>
          </button>
          {menu.target ? (
            (() => {
              const target = menu.target;
              return (
                <>
                  <button
                    type="button"
                    role="menuitem"
                    className="file-tree-context-item"
                    onClick={() => {
                      onRename(target);
                      setMenu(null);
                    }}
                  >
                    <span className="file-tree-context-icon" aria-hidden>
                      <Pencil size={14} strokeWidth={1.75} />
                    </span>
                    <span className="file-tree-context-label">
                      Переименовать…
                    </span>
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    className="file-tree-context-item danger"
                    onClick={() => {
                      onDelete(target);
                      setMenu(null);
                    }}
                  >
                    <span className="file-tree-context-icon" aria-hidden>
                      <Trash2 size={14} strokeWidth={1.75} />
                    </span>
                    <span className="file-tree-context-label">Удалить…</span>
                  </button>
                </>
              );
            })()
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
