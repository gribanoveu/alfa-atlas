import type { BottomTool } from "../../hooks/useWorkspaceLayout";
import { PanelResizeHandle } from "../PanelResizeHandle/PanelResizeHandle";
import { HideIcon } from "../icons/HideIcon";
import "./BottomDock.css";

const TOOLS: { id: BottomTool; label: string; empty: string }[] = [
  {
    id: "suggestions",
    label: "Подсказки",
    empty: "Нет активных подсказок",
  },
  {
    id: "formatting",
    label: "Форматирование",
    empty: "Проблем форматирования нет",
  },
];

type BottomDockProps = {
  activeTool: BottomTool | null;
  onToggleTool: (tool: BottomTool) => void;
  onHide: () => void;
  onResize?: (delta: number) => void;
  onResizeEnd?: () => void;
};

export function BottomDock({
  activeTool,
  onToggleTool,
  onHide,
  onResize,
  onResizeEnd,
}: BottomDockProps) {
  const open = Boolean(activeTool);
  const active = TOOLS.find((tool) => tool.id === activeTool);

  return (
    <aside className={`bottom-dock ${open ? "is-open" : "is-collapsed"}`}>
      {open && onResize ? (
        <PanelResizeHandle
          direction="vertical"
          invert
          ariaLabel="Изменить высоту нижней панели"
          onResize={onResize}
          onResizeEnd={onResizeEnd}
        />
      ) : null}

      {open && active ? (
        <div className="bottom-tool-window">
          <header className="bottom-tool-head">
            <span className="bottom-tool-title">{active.label}</span>
            <button
              type="button"
              className="bottom-tool-hide"
              onClick={onHide}
              title="Hide"
              aria-label={`Скрыть ${active.label}`}
            >
              <HideIcon />
            </button>
          </header>
          <div className="bottom-tool-body">
            <div className="panel-empty">{active.empty}</div>
          </div>
        </div>
      ) : null}

      <nav className="bottom-stripe" aria-label="Bottom tool windows">
        {TOOLS.map((tool) => (
          <button
            key={tool.id}
            type="button"
            className={`bottom-stripe-btn ${activeTool === tool.id ? "active" : ""}`}
            title={tool.label}
            aria-pressed={activeTool === tool.id}
            onClick={() => onToggleTool(tool.id)}
          >
            {tool.label}
          </button>
        ))}
      </nav>
    </aside>
  );
}
