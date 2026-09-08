import { Suspense, lazy, useEffect, useState } from "react";
import { Maximize2 } from "lucide-react";
import { useDiagramTheme } from "../../lib/diagramTheme";
import { makeDiagramBlock } from "../AsciiDocPreview/syntheticBlock";
import { AssistantLoadingBars } from "./AssistantLoadingBars";
import "../AsciiDocPreview/AsciiDocPreview.css";
import "../ToolLog/ToolCallLogModal.css";

// Загружается только при открытии модалки: чат импортируется всегда, а полный
// вьюер с зумом нужен по клику — и это же держит `AscMermaid` вне графа
// импортов тестов, которые подменяют его через process-wide `mock.module`.
const AscMermaid = lazy(() =>
  import("../AsciiDocPreview/AscMermaid").then((m) => ({ default: m.AscMermaid })),
);

type RenderState =
  | { kind: "loading" }
  | { kind: "ok"; svg: string }
  | { kind: "error"; message: string };

/** Полноразмерный просмотр схемы из чата — тот же `AscMermaid` с зумом и
 * панорамированием, что и в превью документов, но в модалке: колонка чата
 * слишком узкая, чтобы разглядывать в ней граф. */
function MermaidModal({ source, onClose }: { source: string; onClose: () => void }) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return (
    <div className="tool-log-backdrop" role="presentation" onClick={onClose}>
      <div
        className="tool-log-dialog markdown-mermaid-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Схема"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="tool-log-header">
          <h2 className="tool-log-title">Схема</h2>
          <div className="tool-log-header-actions">
            <button type="button" className="tool-log-close" onClick={onClose} aria-label="Закрыть">
              ×
            </button>
          </div>
        </header>
        <div className="tool-log-body markdown-mermaid-dialog-body">
          <Suspense fallback={<AssistantLoadingBars />}>
            <AscMermaid block={makeDiagramBlock(source, null)} />
          </Suspense>
        </div>
      </div>
    </div>
  );
}

/** Блок ```mermaid внутри ответа ассистента, отрисованный как схема.
 *
 * Не `AscMermaid` напрямую: у того тулбар и вьюпорт на 320–70vh, а в ленте
 * чата нужен низкий превью-кадр. Зум живёт в модалке по клику. */
export function AssistantMermaidBlock({ source }: { source: string }) {
  const theme = useDiagramTheme();
  const [state, setState] = useState<RenderState>({ kind: "loading" });
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let alive = true;
    setState({ kind: "loading" });
    // Динамически: движки (mermaid ~600 кБ, PlantUML ~6 МБ) не должны
    // попадать в граф импортов всего чата ради блока, которого в ответе
    // обычно нет.
    void import("../../lib/diagramRender")
      .then(({ renderDiagram }) => renderDiagram("mermaid", source, theme))
      .then((result) => {
        if (alive) setState(result);
      });
    return () => {
      alive = false;
    };
  }, [source, theme]);

  if (state.kind === "loading") {
    return (
      <div className="markdown-mermaid is-loading">
        <AssistantLoadingBars />
      </div>
    );
  }

  if (state.kind === "error") {
    return (
      <div className="markdown-mermaid is-error">
        <div className="markdown-mermaid-error-title">Ошибка Mermaid</div>
        <pre className="markdown-mermaid-error-message">{state.message}</pre>
        <pre className="markdown-mermaid-source">{source}</pre>
      </div>
    );
  }

  return (
    <>
      <button
        type="button"
        className="markdown-mermaid"
        onClick={() => setOpen(true)}
        title="Открыть схему"
      >
        <span className="markdown-mermaid-svg" dangerouslySetInnerHTML={{ __html: state.svg }} />
        <span className="markdown-mermaid-zoom-hint" aria-hidden>
          <Maximize2 size={13} />
        </span>
      </button>
      {open ? <MermaidModal source={source} onClose={() => setOpen(false)} /> : null}
    </>
  );
}
