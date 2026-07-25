import { PanelResizeHandle } from "../PanelResizeHandle/PanelResizeHandle";
import { HideIcon } from "../icons/HideIcon";
import "./Sidebar.css";

type SidebarProps = {
  open: boolean;
  onToggle: () => void;
  projectRoot: string | null;
  projectName: string | null;
  onResize?: (delta: number) => void;
  onResizeEnd?: () => void;
};

export function Sidebar({
  open,
  onToggle,
  projectRoot,
  projectName,
  onResize,
  onResizeEnd,
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
        {projectRoot ? (
          <div className="panel-empty">
            <div>{projectName}</div>
            <div className="sidebar-project-path" title={projectRoot}>
              {projectRoot}
            </div>
          </div>
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
