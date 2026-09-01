import { useEffect, useRef, useState } from "react";
import { Loader2 } from "lucide-react";
import type { ToolCallBlock } from "../../lib/chatBlocks";
import { visualFromBlock, type Visual } from "../../lib/visuals";
import { useDiagramTheme } from "../../lib/diagramTheme";
import { renderDiagram } from "../../lib/diagramRender";

/** Title to show before the call settles — the result is not there yet, so
 *  this is the only place the title exists. */
function titleFromArgs(block: ToolCallBlock): string {
  try {
    const args = JSON.parse(block.argumentsJson) as { title?: string };
    return args.title?.trim() || "Схема";
  } catch {
    return "Схема";
  }
}

/** What the model is told when its diagram source does not render. Names
 *  no wire tool (the system prompt bans those in user-facing text, and the
 *  model's reply sits right next to this) and states the engine's own
 *  message, which is what makes the fix mechanical. */
export function renderFailureNote(title: string, message: string): string {
  return `Схема «${title}» не отрисовалась: ${message}. Исправь исходник и нарисуй её заново — пользователь сейчас видит на её месте ошибку.`;
}

type PreviewState =
  | { kind: "loading" }
  | { kind: "ok"; svg: string }
  | { kind: "error"; message: string };

/** Renders the diagram right in the card, through the same engines the
 *  AsciiDoc preview and the visualization tab use.
 *
 *  This is not decoration: `visualize` reports success without the source
 *  ever having been drawn, so before this the only way to discover a
 *  diagram that does not parse was to open the tab. Rendering here is what
 *  turns a broken diagram into something both the user and the model find
 *  out about — see `onRenderError`. */
function VisualPreview({
  visual,
  onRenderError,
}: {
  visual: Visual;
  onRenderError: (message: string) => void;
}) {
  const theme = useDiagramTheme();
  const [state, setState] = useState<PreviewState>({ kind: "loading" });
  // One report per card, ever: the effect re-runs on a theme change, and a
  // second identical note would just spend a round telling the model
  // something it already knows.
  const reported = useRef(false);
  // Held in a ref, and deliberately out of the effect's dependency list:
  // the parent passes a fresh closure on every render, and `setState` below
  // causes a render — depending on it directly would re-render the diagram
  // in a loop.
  const reportRef = useRef(onRenderError);
  reportRef.current = onRenderError;

  const { format, source } = visual.content;
  const title = visual.title;

  useEffect(() => {
    let alive = true;
    setState({ kind: "loading" });
    void renderDiagram(format, source, theme).then((result) => {
      if (!alive) return;
      setState(result);
      if (result.kind === "error" && !reported.current) {
        reported.current = true;
        reportRef.current(result.message);
      }
    });
    return () => {
      alive = false;
    };
    // Primitives only — `visual.content` is rebuilt by `visualFromBlock` on
    // every render and would never compare equal.
  }, [format, source, theme]);

  if (state.kind === "loading") {
    return (
      <div className="assistant-visual-preview is-loading">
        <Loader2 className="assistant-chat-tool-spinner" size={14} aria-hidden />
      </div>
    );
  }
  if (state.kind === "error") {
    return <p className="assistant-plan-card-error">{state.message}</p>;
  }
  return (
    <div
      // Тот же случай, что и в полноразмерном вьюере: PlantUML рисует тёмным
      // по прозрачному и палитру не слушает, поэтому ему нужна светлая
      // подложка. Mermaid перекрашивается сам и остаётся на фоне панели.
      className={`assistant-visual-preview${format === "plantuml" ? " is-plantuml" : ""}`}
      role="img"
      aria-label={title}
      dangerouslySetInnerHTML={{ __html: state.svg }}
    />
  );
}

/** Settled `visualize` card — same visual language as `AssistantPlanCard`
 *  (eyebrow, accent border, shared `assistant-btn`). */
export function AssistantVisualCard({
  block,
  turnActive,
  onOpenVisual,
  onRenderError,
  onRedraw,
}: {
  block: ToolCallBlock;
  /** Whether the turn that produced this card is still running. Only then
   *  can a render failure be handed back to the model mid-turn; afterwards
   *  the note would be dropped at the start of the next turn, so the
   *  «Перерисовать» button is the way. */
  turnActive: boolean;
  onOpenVisual: (visual: Visual) => void;
  onRenderError: (note: string) => void;
  onRedraw: (request: string) => void;
}) {
  const visual = visualFromBlock(block);
  const [renderError, setRenderError] = useState<string | null>(null);

  if (block.status === "running" || block.status === "pendingApproval") {
    return (
      <div className="assistant-plan-card is-running">
        <div className="assistant-plan-card-header">
          <span className="assistant-plan-card-eyebrow">Визуализация</span>
          <div className="assistant-plan-card-title assistant-plan-card-title-live">
            <Loader2 className="assistant-chat-tool-spinner" size={14} aria-hidden />
            Рисую схему…
          </div>
        </div>
      </div>
    );
  }

  // `visual` is null on an errored call and also on a settled one whose
  // arguments no longer parse — from the user's side both are the same
  // thing: there is nothing to open.
  if (block.status === "error" || !visual) {
    return (
      <div className="assistant-plan-card is-error">
        <div className="assistant-plan-card-header">
          <span className="assistant-plan-card-eyebrow">Визуализация</span>
          <div className="assistant-plan-card-title">
            Не удалось построить схему «{titleFromArgs(block)}»
          </div>
        </div>
        {block.errorMessage ? (
          <p className="assistant-plan-card-error">{block.errorMessage}</p>
        ) : null}
      </div>
    );
  }

  const handleRenderError = (message: string) => {
    setRenderError(message);
    if (turnActive) onRenderError(renderFailureNote(visual.title, message));
  };

  const broken = renderError !== null;

  return (
    <div className={`assistant-plan-card${broken ? " is-error" : ""}`}>
      <div className="assistant-plan-card-header">
        <span className="assistant-plan-card-eyebrow">
          {broken ? "Визуализация" : "Схема готова"}
        </span>
        <div className="assistant-plan-card-title">
          {broken ? `Схема «${visual.title}» не отрисовалась` : visual.title}
        </div>
      </div>

      {visual.caption && !broken ? (
        <p className="assistant-plan-card-overview">{visual.caption}</p>
      ) : null}

      <VisualPreview visual={visual} onRenderError={handleRenderError} />

      <div className="assistant-plan-card-actions">
        {broken ? (
          <button
            type="button"
            className="assistant-btn"
            onClick={() => onRedraw(renderFailureNote(visual.title, renderError))}
          >
            Перерисовать
          </button>
        ) : (
          <button type="button" className="assistant-btn" onClick={() => onOpenVisual(visual)}>
            Просмотр
          </button>
        )}
      </div>
    </div>
  );
}

export function isVisualToolBlock(block: ToolCallBlock): boolean {
  return block.name === "visualize";
}
