import {
  ClipboardPaste,
  Copy,
  ExternalLink,
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
  type DragEvent as ReactDragEvent,
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
  onMove: (source: FileTreeDeleteTarget, destDirPath: string) => void;
  onRevealInExplorer: (path: string) => void;
  onCopy: (target: FileTreeDeleteTarget) => void;
  onPaste: (destDirPath: string) => void;
  copiedItem: FileTreeDeleteTarget | null;
  onResizeExternal?: (delta: number) => void;
  onResizeExternalEnd?: () => void;
};

type FileTreeNodeProps = {
  node: TreeNode;
  depth: number;
  activePath: string | null;
  expandedDirs: ReadonlySet<string>;
  dragging: FileTreeDeleteTarget | null;
  dropTargetPath: string | null;
  onToggleDir: (path: string) => void;
  onOpenFile: (path: string) => void;
  onContextMenu: (
    event: ReactMouseEvent,
    parentPath: string,
    target: FileTreeDeleteTarget | null,
  ) => void;
  onDragStart: (event: ReactDragEvent, source: FileTreeDeleteTarget) => void;
  onDragEnd: () => void;
  onDragOverDir: (event: ReactDragEvent, dirPath: string) => void;
  onDragLeaveDir: (event: ReactDragEvent, dirPath: string) => void;
  onDropOnDir: (event: ReactDragEvent, dirPath: string) => void;
};

type ContextMenuState = {
  x: number;
  y: number;
  parentPath: string;
  target: FileTreeDeleteTarget | null;
};

const MENU_WIDTH = 200;
const MENU_HEIGHT = 184;

function parentOfFile(path: string): string {
  const parts = path.split(/[/\\]/);
  if (parts.length <= 1) return ".";
  return parts.slice(0, -1).join("/");
}

/** Whether `dirPath` is the same as `source.path` or one of its descendants. */
function isSelfOrDescendant(source: FileTreeDeleteTarget, dirPath: string): boolean {
  if (!source.isDir) return false;
  if (dirPath === source.path) return true;
  return dirPath.startsWith(source.path + "/");
}

/** A drop is valid only when the destination parent actually changes. */
function isNoOpMove(source: FileTreeDeleteTarget, destDirPath: string): boolean {
  return parentOfFile(source.path) === destDirPath;
}

function isValidDrop(source: FileTreeDeleteTarget | null, destDirPath: string): boolean {
  if (!source) return false;
  if (destDirPath === source.path) return false;
  if (isSelfOrDescendant(source, destDirPath)) return false;
  if (isNoOpMove(source, destDirPath)) return false;
  return true;
}

function FileTreeNode({
  node,
  depth,
  activePath,
  expandedDirs,
  dragging,
  dropTargetPath,
  onToggleDir,
  onOpenFile,
  onContextMenu,
  onDragStart,
  onDragEnd,
  onDragOverDir,
  onDragLeaveDir,
  onDropOnDir,
}: FileTreeNodeProps) {
  if (node.isDir) {
    const expanded = expandedDirs.has(node.path);
    const isDragging = dragging?.path === node.path;
    const isDropTarget = dropTargetPath === node.path;
    return (
      <div className="file-tree-branch">
        <button
          type="button"
          className={
            "file-tree-row dir" +
            (isDragging ? " dragging" : "") +
            (isDropTarget ? " drop-target" : "")
          }
          style={{ paddingLeft: 4 + depth * 14 }}
          draggable
          onDragStart={(event) => onDragStart(event, { path: node.path, isDir: true })}
          onDragEnd={onDragEnd}
          onDragOver={(event) => onDragOverDir(event, node.path)}
          onDragLeave={(event) => onDragLeaveDir(event, node.path)}
          onDrop={(event) => onDropOnDir(event, node.path)}
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
                dragging={dragging}
                dropTargetPath={dropTargetPath}
                onToggleDir={onToggleDir}
                onOpenFile={onOpenFile}
                onContextMenu={onContextMenu}
                onDragStart={onDragStart}
                onDragEnd={onDragEnd}
                onDragOverDir={onDragOverDir}
                onDragLeaveDir={onDragLeaveDir}
                onDropOnDir={onDropOnDir}
              />
            ))
          : null}
      </div>
    );
  }

  const active = activePath === node.path;
  const isDragging = dragging?.path === node.path;
  return (
    <button
      type="button"
      className={
        "file-tree-row file" +
        (active ? " active" : "") +
        (isDragging ? " dragging" : "")
      }
      style={{ paddingLeft: 4 + depth * 14 }}
      draggable
      onDragStart={(event) => onDragStart(event, { path: node.path, isDir: false })}
      onDragEnd={onDragEnd}
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
  onMove,
  onRevealInExplorer,
  onCopy,
  onPaste,
  copiedItem,
  onResizeExternal,
  onResizeExternalEnd,
}: FileTreeProps) {
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState<FileTreeDeleteTarget | null>(null);
  const [dropTargetPath, setDropTargetPath] = useState<string | null>(null);
  const draggingRef = useRef<FileTreeDeleteTarget | null>(null);
  const rootExpanded = expandedDirs.has(".");
  const { main, external } = separateExternal
    ? splitExternalNodes(nodes)
    : { main: nodes, external: null };
  const externalExpanded = external
    ? expandedDirs.has(external.path)
    : false;
  const dockedExternal = Boolean(external);

  const handleDragStart = (
    event: ReactDragEvent,
    source: FileTreeDeleteTarget,
  ) => {
    event.stopPropagation();
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", source.path);
    draggingRef.current = source;
    setDragging(source);
  };

  const handleDragEnd = () => {
    draggingRef.current = null;
    setDragging(null);
    setDropTargetPath(null);
  };

  const handleDragOverDir = (event: ReactDragEvent, dirPath: string) => {
    const source = draggingRef.current;
    if (!isValidDrop(source, dirPath)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    setDropTargetPath(dirPath);
  };

  const handleDragLeaveDir = (event: ReactDragEvent, dirPath: string) => {
    // Only clear when leaving the row entirely (not when entering a child element).
    const related = event.relatedTarget as Node | null;
    if (related && event.currentTarget.contains(related)) return;
    setDropTargetPath((current) => (current === dirPath ? null : current));
  };

  const handleDropOnDir = (event: ReactDragEvent, dirPath: string) => {
    const source = draggingRef.current;
    if (!isValidDrop(source, dirPath)) {
      setDropTargetPath(null);
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    draggingRef.current = null;
    setDragging(null);
    setDropTargetPath(null);
    if (source) onMove(source, dirPath);
  };

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
            className={
              "file-tree-row dir root" +
              (dropTargetPath === "." ? " drop-target" : "")
            }
            style={{ paddingLeft: 4 }}
            title={rootPath}
            onDragOver={(event) => handleDragOverDir(event, ".")}
            onDragLeave={(event) => handleDragLeaveDir(event, ".")}
            onDrop={(event) => handleDropOnDir(event, ".")}
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
                  dragging={dragging}
                  dropTargetPath={dropTargetPath}
                  onToggleDir={onToggleDir}
                  onOpenFile={onOpenFile}
                  onContextMenu={openContextMenu}
                  onDragStart={handleDragStart}
                  onDragEnd={handleDragEnd}
                  onDragOverDir={handleDragOverDir}
                  onDragLeaveDir={handleDragLeaveDir}
                  onDropOnDir={handleDropOnDir}
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
                className={
                  "file-tree-row dir external-root" +
                  (dropTargetPath === external.path ? " drop-target" : "") +
                  (dragging?.path === external.path ? " dragging" : "")
                }
                style={{ paddingLeft: 4 }}
                title={external.path}
                draggable
                onDragStart={(event) =>
                  handleDragStart(event, { path: external.path, isDir: true })
                }
                onDragEnd={handleDragEnd}
                onDragOver={(event) => handleDragOverDir(event, external.path)}
                onDragLeave={(event) => handleDragLeaveDir(event, external.path)}
                onDrop={(event) => handleDropOnDir(event, external.path)}
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
                      dragging={dragging}
                      dropTargetPath={dropTargetPath}
                      onToggleDir={onToggleDir}
                      onOpenFile={onOpenFile}
                      onContextMenu={openContextMenu}
                      onDragStart={handleDragStart}
                      onDragEnd={handleDragEnd}
                      onDragOverDir={handleDragOverDir}
                      onDragLeaveDir={handleDragLeaveDir}
                      onDropOnDir={handleDropOnDir}
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
          {menu.target === null ? (
            <button
              type="button"
              role="menuitem"
              className="file-tree-context-item"
              onClick={() => {
                onRevealInExplorer(menu.parentPath);
                setMenu(null);
              }}
            >
              <span className="file-tree-context-icon" aria-hidden>
                <ExternalLink size={14} strokeWidth={1.75} />
              </span>
              <span className="file-tree-context-label">
                Открыть в проводнике
              </span>
            </button>
          ) : null}
          {copiedItem &&
          !(copiedItem.isDir && isSelfOrDescendant(copiedItem, menu.parentPath)) ? (
            <>
              <div className="file-tree-context-sep" role="separator" />
              <button
                type="button"
                role="menuitem"
                className="file-tree-context-item"
                onClick={() => {
                  onPaste(menu.parentPath);
                  setMenu(null);
                }}
              >
                <span className="file-tree-context-icon" aria-hidden>
                  <ClipboardPaste size={14} strokeWidth={1.75} />
                </span>
                <span className="file-tree-context-label">Вставить</span>
              </button>
            </>
          ) : null}
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
                      onRevealInExplorer(target.path);
                      setMenu(null);
                    }}
                  >
                    <span className="file-tree-context-icon" aria-hidden>
                      <ExternalLink size={14} strokeWidth={1.75} />
                    </span>
                    <span className="file-tree-context-label">
                      Открыть в проводнике
                    </span>
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    className="file-tree-context-item"
                    onClick={() => {
                      onCopy(target);
                      setMenu(null);
                    }}
                  >
                    <span className="file-tree-context-icon" aria-hidden>
                      <Copy size={14} strokeWidth={1.75} />
                    </span>
                    <span className="file-tree-context-label">
                      Копировать
                    </span>
                  </button>
                  <div className="file-tree-context-sep" role="separator" />
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
