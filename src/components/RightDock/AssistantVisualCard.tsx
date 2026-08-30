import { Loader2 } from "lucide-react";
import type { ToolCallBlock } from "../../lib/chatBlocks";
import { visualFromBlock, type Visual } from "../../lib/visuals";

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

/** Settled `visualize` card — same visual language as `AssistantPlanCard`
 *  (eyebrow, accent border, shared `assistant-btn`). */
export function AssistantVisualCard({
  block,
  onOpenVisual,
}: {
  block: ToolCallBlock;
  onOpenVisual: (visual: Visual) => void;
}) {
  const visual = visualFromBlock(block);

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

  return (
    <div className="assistant-plan-card">
      <div className="assistant-plan-card-header">
        <span className="assistant-plan-card-eyebrow">Схема готова</span>
        <div className="assistant-plan-card-title">{visual.title}</div>
      </div>

      {visual.caption ? <p className="assistant-plan-card-overview">{visual.caption}</p> : null}

      <div className="assistant-plan-card-actions">
        <button type="button" className="assistant-btn" onClick={() => onOpenVisual(visual)}>
          Просмотр
        </button>
      </div>
    </div>
  );
}

export function isVisualToolBlock(block: ToolCallBlock): boolean {
  return block.name === "visualize";
}
