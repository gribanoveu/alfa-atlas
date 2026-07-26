import { PanelResizeHandle } from "../PanelResizeHandle/PanelResizeHandle";
import { HideIcon } from "../icons/HideIcon";
import type { TreeNode } from "../../lib/project";
import { FileTree, type FileTreeDeleteTarget } from "./FileTree";
import "./Sidebar.css";

type SidebarProps = {
  open: boolean;
  onToggle: () => void;
  docsRoot: string | null;
  tree: TreeNode[];
  treeLoading: boolean;
  treeError: string | null;
  activePath: string | null;
  expandedDirs: ReadonlySet<string>;
  separateExternal?: boolean;
  onToggleDir: (path: string) => void;
  onOpenFile: (path: string) => void;
  onNewFile: (parentPath: string) => void;
  onNewFolder: (parentPath: string) => void;
  onDelete: (target: FileTreeDeleteTarget) => void;
  onResize?: (delta: number) => void;
  onResizeEnd?: () => void;
  onResizeExternal?: (delta: number) => void;
  onResizeExternalEnd?: () => void;
};

function rootNameOf(docsRoot: string): string {
  return docsRoot.split(/[/\\]/).filter(Boolean).pop() ?? "Документация";
}

export function Sidebar({
  open,
  onToggle,
  docsRoot,
  tree,
  treeLoading,
  treeError,
  activePath,
  expandedDirs,
  separateExternal = true,
  onToggleDir,
  onOpenFile,
  onNewFile,
  onNewFolder,
  onDelete,
  onResize,
  onResizeEnd,
  onResizeExternal,
  onResizeExternalEnd,
}: SidebarProps) {
  if (!open) {
    return (
      <aside className="sidebar sidebar-collapsed">
        <button
          type="button"
          className="sidebar-rail-toggle"
          onClick={onToggle}
          aria-expanded={false}
          title="Показать документацию"
        >
          <span className="sidebar-rail-label">Документация</span>
        </button>
      </aside>
    );
  }

  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <span>Документация</span>
        <div className="icons">
          <button
            type="button"
            className="sidebar-hide"
            onClick={onToggle}
            title="Hide"
            aria-label="Скрыть панель документации"
          >
            <HideIcon />
          </button>
        </div>
      </div>
      <div className="sidebar-body">
        {docsRoot ? (
          treeLoading ? (
            <div className="panel-empty">Загрузка…</div>
          ) : treeError ? (
            <div className="panel-empty">{treeError}</div>
          ) : (
            <FileTree
              nodes={tree}
              rootName={rootNameOf(docsRoot)}
              rootPath={docsRoot}
              activePath={activePath}
              expandedDirs={expandedDirs}
              separateExternal={separateExternal}
              onToggleDir={onToggleDir}
              onOpenFile={onOpenFile}
              onNewFile={onNewFile}
              onNewFolder={onNewFolder}
              onDelete={onDelete}
              onResizeExternal={onResizeExternal}
              onResizeExternalEnd={onResizeExternalEnd}
            />
          )
        ) : (
          <div className="panel-empty">Нет открытого репозитория</div>
        )}
      </div>
      {onResize ? (
        <PanelResizeHandle
          direction="horizontal"
          ariaLabel="Изменить ширину панели документации"
          onResize={onResize}
          onResizeEnd={onResizeEnd}
        />
      ) : null}
    </aside>
  );
}
