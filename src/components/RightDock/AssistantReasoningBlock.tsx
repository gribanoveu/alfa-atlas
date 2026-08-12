import { Brain, ChevronDown, ChevronRight } from "lucide-react";
import { useState } from "react";
import type { ReasoningBlock } from "../../lib/chatBlocks";

type AssistantReasoningBlockProps = {
  block: ReasoningBlock;
  /** Whether the model is still producing this block's text right now —
   * computed by the caller exactly like `AssistantMarkdown`'s own
   * `streaming` prop (last block + message still `streaming`), not stored
   * on the block itself. Drives the shimmering label; once `content` starts
   * arriving this block is no longer last, so this flips to `false` on its
   * own without any explicit "reasoning done" event. */
  thinking: boolean;
};

/** Collapsed-by-default disclosure for a reasoning-capable model's
 * "thinking" text, matching `AssistantToolCallBlock`'s interaction pattern.
 * `expanded` is local state driven only by the user's own click — the
 * `thinking` prop flipping `true → false` when the model moves on to its
 * actual answer never touches it, so a block the user already opened stays
 * open (just its header stops shimmering and its label changes), instead of
 * springing shut under them. */
export function AssistantReasoningBlock({ block, thinking }: AssistantReasoningBlockProps) {
  const [expanded, setExpanded] = useState(false);
  const Chevron = expanded ? ChevronDown : ChevronRight;

  return (
    <div className={`assistant-reasoning ${thinking ? "assistant-reasoning-thinking" : "assistant-reasoning-done"}`}>
      <button
        type="button"
        className="assistant-reasoning-header"
        aria-expanded={expanded}
        onClick={() => setExpanded((v) => !v)}
      >
        <Chevron className="assistant-reasoning-chevron" size={12} aria-hidden />
        <Brain className="assistant-reasoning-icon" size={13} aria-hidden />
        <span className="assistant-reasoning-label">{thinking ? "Модель думает…" : "Ход рассуждений"}</span>
      </button>

      {expanded ? <div className="assistant-reasoning-detail">{block.content}</div> : null}
    </div>
  );
}
