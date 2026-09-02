import { Brain, ChevronDown, ChevronRight } from "lucide-react";
import { useState } from "react";
import type { ReasoningBlock } from "../../lib/chatBlocks";
import { AssistantElapsedTimer } from "./AssistantElapsedTimer";

/** Rotating stand-ins for the old, fixed «Модель думает…». Subjectless third
 * person throughout — mixing in a first-person «Обдумываю…» would read as two
 * different speakers on consecutive cards. Kept short: the label is
 * `white-space: nowrap` with an ellipsis, so anything past ~22 characters
 * gets clipped in a narrow dock. None of them claims the model is *writing*
 * — `AssistantThinkingIndicator` reuses this list for the silent gap after a
 * settled tool call, where no prose is being produced at all. */
const THINKING_PHRASES = [
  "Обдумывает задачу…",
  "Взвешивает варианты…",
  "Прикидывает план…",
  "Собирается с мыслями…",
  "Изучает контекст…",
  "Ищет подход…",
  "Разбирается в деталях…",
  "Сопоставляет факты…",
  "Складывает картину…",
  "Проверяет догадку…",
  "Строит цепочку мыслей…",
  "Ищет зацепку…",
] as const;

/** Deliberately called from a `useState` *initializer*, never during render:
 * these cards re-render on every streamed token, and re-rolling the phrase
 * each time would strobe the label several times a second. One phrase is
 * picked when a card mounts and holds for that card's whole thinking phase;
 * the variety comes from the next card, one tool call later. */
function pickThinkingPhrase(): string {
  return THINKING_PHRASES[Math.floor(Math.random() * THINKING_PHRASES.length)]!;
}

/** Size of a reasoning trace, for the collapsed header.
 *
 * The whole point is a number that visibly moves while the card stays shut:
 * the shimmer says "busy", this says "busy *and getting somewhere*". So it
 * has to stay narrow — the label beside it is `white-space: nowrap` with an
 * ellipsis, and every character here is one the label loses in a narrow
 * dock. Exact below a thousand, one decimal above, no decimal past ten
 * thousand.
 *
 * `null` for an empty trace: a "0 зн." that flashes before the first delta
 * says less than showing nothing.
 *
 * Counts UTF-16 code units, not glyphs — `String.length`. A reasoning trace
 * is prose, so the difference only shows up on emoji, and the number is
 * meant to be watched rather than read off. */
export function formatReasoningSize(chars: number): string | null {
  if (chars < 1) return null;
  if (chars < 1000) return `${chars} зн.`;
  const thousands = chars / 1000;
  const value =
    thousands < 10 ? thousands.toFixed(1).replace(".", ",") : Math.round(thousands).toString();
  return `${value}к зн.`;
}

type AssistantReasoningBlockProps = {
  block: ReasoningBlock;
  /** Whether the model is still producing this block's text right now —
   * computed by the caller exactly like `AssistantMarkdown`'s own
   * `streaming` prop (`openStreamingBlockIds` plus the message's own
   * `streaming` flag), not stored on the block itself. Drives the shimmering
   * label; the next tool call closes this block off, so it flips to `false`
   * on its own without any explicit "reasoning done" event. */
  thinking: boolean;
};

/** Collapsed-by-default disclosure for a reasoning-capable model's
 * "thinking" text, matching `AssistantToolCallBlock`'s interaction pattern.
 * `expanded` is local state driven only by the user's own click — the
 * `thinking` prop flipping `true → false` when the model moves on to its
 * actual answer never touches it, so a block the user already opened stays
 * open (just its header stops shimmering and its label changes), instead of
 * springing shut under them. */
/** Shown while a stream is in flight but nothing is growing yet — empty
 * first round, or the gap after a settled tool call before the next
 * tokens. Same chrome as a live reasoning block so every provider, not
 * just ones that send `reasoning_content`, has a visible "thinking" card. */
export function AssistantThinkingIndicator() {
  const [phrase] = useState(pickThinkingPhrase);

  return (
    <div className="assistant-reasoning assistant-reasoning-thinking" role="status" aria-label={phrase}>
      <div className="assistant-reasoning-header assistant-reasoning-header-static">
        <Brain className="assistant-reasoning-icon" size={13} aria-hidden />
        <span className="assistant-reasoning-label">{phrase}</span>
        <AssistantElapsedTimer running className="assistant-reasoning-elapsed" />
      </div>
    </div>
  );
}

export function AssistantReasoningBlock({ block, thinking }: AssistantReasoningBlockProps) {
  const [expanded, setExpanded] = useState(false);
  // Captured once: a block restored from persisted history starts with
  // `thinking: false` and never had a live "thinking" phase in this
  // session, so it must never show a fabricated "Thought for 0s" timer.
  const [wasThinking] = useState(thinking);
  const [phrase] = useState(pickThinkingPhrase);
  const Chevron = expanded ? ChevronDown : ChevronRight;
  const size = formatReasoningSize(block.content.length);

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
        <span className="assistant-reasoning-label">{thinking ? phrase : "Ход рассуждений"}</span>
        {wasThinking ? (
          <AssistantElapsedTimer running={thinking} className="assistant-reasoning-elapsed" />
        ) : null}
        {/* Unlike the timer, this needs no `wasThinking` guard: the text it
            measures is really there in a block restored from history, so
            showing its size is a fact rather than a fabricated duration. */}
        {size ? <span className="assistant-reasoning-chars">{size}</span> : null}
      </button>

      {expanded ? <div className="assistant-reasoning-detail">{block.content}</div> : null}
    </div>
  );
}
