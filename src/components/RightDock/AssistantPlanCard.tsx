import type { ToolCallBlock } from "../../lib/chatBlocks";

function planIdFromBlock(block: ToolCallBlock): string | null {
  if (block.status !== "done" || !block.result) return null;
  if (block.result.tool === "planCreated" || block.result.tool === "planUpdated") {
    return block.result.result.planId;
  }
  return null;
}

function nameFromBlock(block: ToolCallBlock): string {
  if (block.status === "done" && block.result) {
    if (block.result.tool === "planCreated" || block.result.tool === "planUpdated") {
      return block.result.result.name;
    }
  }
  try {
    const args = JSON.parse(block.argumentsJson) as { name?: string };
    return args.name ?? "План";
  } catch {
    return "План";
  }
}

function overviewFromBlock(block: ToolCallBlock): string {
  if (block.status === "done" && block.result) {
    if (block.result.tool === "planCreated" || block.result.tool === "planUpdated") {
      return block.result.result.overview;
    }
  }
  try {
    const args = JSON.parse(block.argumentsJson) as { overview?: string };
    return args.overview ?? "";
  } catch {
    return "";
  }
}

/** Settled `createPlan` / `updatePlan` card — same visual language as
 * `AssistantAskUserCard` (eyebrow, accent border, shared `assistant-btn`). */
export function AssistantPlanCard({
  block,
  onOpenPlan,
  onStartPlan,
  startDisabled,
}: {
  block: ToolCallBlock;
  onOpenPlan: (planId: string) => void;
  onStartPlan: (planId: string) => void;
  startDisabled?: boolean;
}) {
  const planId = planIdFromBlock(block);
  const name = nameFromBlock(block);
  const overview = overviewFromBlock(block);
  const eyebrow =
    block.name === "updatePlan" ? "План обновлён" : "План готов";

  if (block.status === "running") {
    return (
      <div className="assistant-plan-card is-running">
        <div className="assistant-plan-card-header">
          <span className="assistant-plan-card-eyebrow">План</span>
          <div className="assistant-plan-card-title">Составляю план…</div>
        </div>
      </div>
    );
  }

  if (block.status === "error" || !planId) {
    return (
      <div className="assistant-plan-card is-error">
        <div className="assistant-plan-card-header">
          <span className="assistant-plan-card-eyebrow">План</span>
          <div className="assistant-plan-card-title">Не удалось сохранить план</div>
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
        <span className="assistant-plan-card-eyebrow">{eyebrow}</span>
        <div className="assistant-plan-card-title">{name}</div>
      </div>

      {overview ? <p className="assistant-plan-card-overview">{overview}</p> : null}

      <div className="assistant-plan-card-actions">
        <button type="button" className="assistant-btn" onClick={() => onOpenPlan(planId)}>
          Открыть план
        </button>
        <button
          type="button"
          className="assistant-btn primary"
          disabled={startDisabled}
          onClick={() => onStartPlan(planId)}
        >
          Начать
        </button>
      </div>
    </div>
  );
}

export function isPlanToolBlock(block: ToolCallBlock): boolean {
  return block.name === "createPlan" || block.name === "updatePlan";
}
