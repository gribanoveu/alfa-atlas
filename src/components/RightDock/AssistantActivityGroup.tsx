import { AlertCircle, ChevronDown, ChevronRight, Sparkles } from "lucide-react";
import { useState } from "react";
import { describeToolActivity } from "../../lib/assistantConfig";
import type { MessageBlock } from "../../lib/chatBlocks";
import { AssistantLoadingBars } from "./AssistantLoadingBars";
import {
  AssistantReasoningBlock,
  THINKING_PHRASES,
} from "./AssistantReasoningBlock";
import { AssistantToolCallBlock } from "./AssistantToolCallBlock";

/** Same reasoning as `AssistantReasoningBlock`'s own phrase pick: called
 * from a `useState` initializer so a card that re-renders on every streamed
 * token does not strobe its label several times a second. */
function pickThinkingPhrase(): string {
  return THINKING_PHRASES[Math.floor(Math.random() * THINKING_PHRASES.length)]!;
}

/** «5 шагов» — Russian needs the real three-form rule here; a fixed
 * «шагов» reads as broken on 1 and 2, which are the common counts. */
export function formatStepCount(count: number): string {
  const mod100 = count % 100;
  const mod10 = count % 10;
  const noun =
    mod100 >= 11 && mod100 <= 14
      ? "шагов"
      : mod10 === 1
        ? "шаг"
        : mod10 >= 2 && mod10 <= 4
          ? "шага"
          : "шагов";
  return `${count} ${noun}`;
}

/** What the collapsed line says while the run is still going: whatever is
 * happening *right now*, so one line replaces the growing list without
 * hiding progress.
 *
 * The newest running call wins — a round can have several in flight, and
 * the most recently started one is the one the user is waiting on. With
 * none running the model is between calls, thinking, which is what the
 * phrase says. */
function liveLabel(blocks: MessageBlock[], phrase: string): string {
  for (let i = blocks.length - 1; i >= 0; i--) {
    const block = blocks[i]!;
    if (block.type === "toolCall" && block.status === "running") {
      return describeToolActivity(block.name, block.argumentsJson);
    }
  }
  return phrase;
}

type AssistantActivityGroupProps = {
  blocks: MessageBlock[];
  /** Whether this run is the tail of a turn still in flight. Drives the
   * shimmer and the live label; computed by the transcript from the
   * message's own `streaming` flag, never stored on a block. */
  active: boolean;
  /** Ids of blocks the model is still writing into, and which of text or
   * reasoning it is writing — passed straight through to the reasoning
   * cards inside, exactly as an ungrouped block would receive them. */
  liveBlockIds: ReadonlySet<string>;
  liveKind: "text" | "reasoning" | undefined;
};

/** One line standing in for a whole run of "the assistant is working":
 * private reasoning and the ordinary tool calls it makes, in order.
 *
 * A turn that reads six files used to put six cards plus its reasoning
 * straight into the transcript, so the answer arrived below a wall of
 * machinery nobody reads twice. Collapsed, the run is one row — shimmering
 * and naming the current step while it runs, a step count once it is done —
 * and opening it renders exactly the cards that were there before, each
 * with its own expandable detail. Nothing is dropped, only folded.
 *
 * Collapsed by default including in loaded history, with one deliberate
 * exception: a run containing a failed call says so on the closed row, so a
 * silent tool error is never something the user has to go looking for. */
export function AssistantActivityGroup({
  blocks,
  active,
  liveBlockIds,
  liveKind,
}: AssistantActivityGroupProps) {
  const [expanded, setExpanded] = useState(false);
  const [phrase] = useState(pickThinkingPhrase);
  const Chevron = expanded ? ChevronDown : ChevronRight;

  const steps = blocks.filter((block) => block.type === "toolCall").length;
  const errors = blocks.filter(
    (block) => block.type === "toolCall" && block.status === "error",
  ).length;

  return (
    <div
      className={`assistant-activity${active ? " assistant-activity-live" : ""}`}
    >
      <button
        type="button"
        className="assistant-activity-header"
        aria-expanded={expanded}
        onClick={() => setExpanded((v) => !v)}
      >
        <Chevron className="assistant-activity-chevron" size={12} aria-hidden />
        {active ? (
          <AssistantLoadingBars />
        ) : (
          <Sparkles className="assistant-activity-icon" size={13} aria-hidden />
        )}
        <span className="assistant-activity-label">
          {active ? liveLabel(blocks, phrase) : "Ход работы"}
        </span>
        {errors > 0 ? (
          <span
            className="assistant-activity-errors"
            title="Часть вызовов завершилась ошибкой — раскройте, чтобы посмотреть"
          >
            <AlertCircle size={12} aria-hidden />
            {errors}
          </span>
        ) : null}
        {steps > 0 ? (
          <span className="assistant-activity-count">{formatStepCount(steps)}</span>
        ) : null}
      </button>

      {expanded ? (
        <div className="assistant-activity-detail">
          {blocks.map((block) =>
            block.type === "reasoning" ? (
              <AssistantReasoningBlock
                key={block.id}
                block={block}
                thinking={liveBlockIds.has(block.id) && liveKind === "reasoning"}
              />
            ) : block.type === "toolCall" ? (
              <AssistantToolCallBlock key={block.id} block={block} />
            ) : null,
          )}
        </div>
      ) : null}
    </div>
  );
}
