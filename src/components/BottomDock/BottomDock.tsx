import type { BottomTool } from "../../hooks/useWorkspaceLayout";
import type { Diagnostic } from "../../lib/workspaceIndex";
import { PanelResizeHandle } from "../PanelResizeHandle/PanelResizeHandle";
import { HideIcon } from "../icons/HideIcon";
import { ProblemsPanel } from "./ProblemsPanel";
import "./BottomDock.css";

type ToolMeta = { id: BottomTool; label: string; empty: string };

const TOOLS: ToolMeta[] = [
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
  {
    id: "problems",
    label: "Проблемы",
    empty: "Нет проблем в индексе",
  },
];

type BottomDockProps = {
  activeTool: BottomTool | null;
  onToggleTool: (tool: BottomTool) => void;
  onHide: () => void;
  onResize?: (delta: number) => void;
  onResizeEnd?: () => void;
  diagnostics: Diagnostic[];
  activeDocumentId: string | null;
  onOpenDiagnostic: (
    documentId: string,
    line: number,
    column: number,
  ) => void;
};

export function BottomDock({
  activeTool,
  onToggleTool,
  onHide,
  onResize,
  onResizeEnd,
  diagnostics,
  activeDocumentId,
  onOpenDiagnostic,
}: BottomDockProps) {
  const open = Boolean(activeTool);
  const active = TOOLS.find((tool) => tool.id === activeTool);

  const errorCount = diagnostics.filter((d) => d.severity === "error").length;
  const warningCount = diagnostics.filter((d) => d.severity === "warning")
    .length;

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
            {active.id === "problems" ? (
              <ProblemsPanel
                diagnostics={diagnostics}
                activeDocumentId={activeDocumentId}
                onOpenDiagnostic={onOpenDiagnostic}
              />
            ) : (
              <div className="panel-empty">{active.empty}</div>
            )}
          </div>
        </div>
      ) : null}

      <nav className="bottom-stripe" aria-label="Bottom tool windows">
        {TOOLS.map((tool) => {
          const isProblems = tool.id === "problems";
          const badge =
            isProblems && (errorCount > 0 || warningCount > 0)
              ? errorCount > 0
                ? `${errorCount}`
                : `${warningCount}`
              : null;
          return (
            <button
              key={tool.id}
              type="button"
              className={`bottom-stripe-btn ${activeTool === tool.id ? "active" : ""} ${isProblems && errorCount > 0 ? "has-errors" : ""}`}
              title={tool.label}
              aria-pressed={activeTool === tool.id}
              onClick={() => onToggleTool(tool.id)}
            >
              {tool.label}
              {badge !== null ? (
                <span
                  className={`bottom-stripe-badge ${errorCount > 0 ? "errors" : "warnings"}`}
                >
                  {badge}
                </span>
              ) : null}
            </button>
          );
        })}
      </nav>
    </aside>
  );
}
