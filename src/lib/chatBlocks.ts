import { normalizeSemanticSearchResult, type ToolResult } from "./aiTools";
import { STEERING_PREFIX, type ChatUsage } from "./llm";
import { estimateTokenCount, estimateTokensFromChars } from "./tokens";

/** One piece of an assistant message's transcript, in chronological order —
 * a run of streamed prose, or one tool invocation with its eventual
 * outcome. Mirrors how Cursor/Claude Code's own CLI render a turn: text,
 * then a tool-call chip, then more text is a normal, expected sequence, and
 * every block is permanent once appended — nothing here is ever cleared or
 * discarded (contrast the old, removed `toolActivity`/
 * `CHAT_STREAM_RESET_EVENT` behavior, which treated tool status as
 * transient UI and threw pre-tool-call prose away).
 *
 * Forward-compat note: this is plain, JSON-serializable data (no functions,
 * no class instances) — a future "save this conversation" feature can
 * serialize a `ChatMessage[]` as-is; nothing here needs reworking for that. */
export type TextBlock = {
  type: "text";
  id: string;
  content: string;
  /** Set once the round that was writing this block has ended (see
   * `closeOpenBlocks`), so a later round's deltas start a new block instead
   * of being appended to this one. Absent on a block that is still open,
   * and on every block recorded before round boundaries were reported. */
  closed?: true;
};

/** A reasoning-capable model's "thinking" text (`reasoning_content` on the
 * wire), usually streamed ahead of the model's actual answer — though some
 * providers interleave the two chunk by chunk, in which case this block and
 * the `TextBlock` below it grow side by side. Closed off by the round's next
 * tool call (or the end of the turn), not by the first `content` delta, so
 * "is this block still growing" is never stored on the block itself: it's
 * derived the same way a `TextBlock`'s own streaming cursor is, from
 * `openStreamingBlockIds` plus the message's own `streaming` flag — see
 * `AssistantConversation`. */
export type ReasoningBlock = {
  type: "reasoning";
  id: string;
  content: string;
  /** Same as `TextBlock["closed"]`. */
  closed?: true;
};

export type SteerBlock = {
  type: "steer";
  id: string;
  text: string;
};

export type ToolCallStatus = "pendingApproval" | "running" | "done" | "error";

/** `id` is the model's own `LlmToolCall.id` off the wire (see
 * `LlmToolCallEvent.id`/`LlmToolResultEvent.id`), not a freshly generated
 * uuid — so a later `TOOL_RESULT_EVENT` can find and settle the exact block
 * a `TOOL_CALL_EVENT` created, regardless of how many other blocks have
 * been appended since (it also doubles as the React list key). */
export type ToolCallBlock = {
  type: "toolCall";
  id: string;
  name: string;
  argumentsJson: string;
  status: ToolCallStatus;
  result?: ToolResult;
  errorMessage?: string;
  /** Set when this call skipped the approval card because the user had
   * already ticked "don't ask again this conversation" for this tool —
   * purely a display hint (the backend event stream is identical either
   * way), so the transcript can still show it was auto-approved rather than
   * looking like it silently ran with no review at all. */
  autoApproved?: boolean;
  /** `Date.now()`-comparable deadline for a `"pendingApproval"` block — the
   * card's countdown strip animates toward it, and `useLlmChat` auto-denies
   * the call once it passes without a manual decision. Only ever set while
   * `status === "pendingApproval"`. */
  deadlineAt?: number;
  /** Set only while `status === "pendingApproval"` — every block created
   * from one paused round's `collectDecisions()` call shares one generated
   * id, letting `groupBlocksForRender` regroup them into a single combined
   * approval card instead of one card per call. Cleared (like `deadlineAt`)
   * once the block transitions away from `"pendingApproval"`. */
  approvalGroupId?: string;
};

export type MessageBlock = TextBlock | ToolCallBlock | ReasoningBlock | SteerBlock;

/** True when the transcript already tells the user the model is busy
 * (growing prose, visible reasoning, or a tool still in flight). False
 * after a settled tool call / empty transcript — that's when the chat
 * must show its thinking card, for every provider, not only those that
 * send `reasoning_content`.
 *
 * Asks `openStreamingBlockIds` rather than looking only at the array's last
 * block: with an interleaving provider the open reasoning block can sit
 * above an open text block, and both are live. */
export function lastBlockShowsLiveProgress(blocks: MessageBlock[]): boolean {
  const last = blocks[blocks.length - 1];
  if (!last) return false;
  const open = openStreamingBlockIds(blocks);
  if (open.size > 0) {
    return blocks.some(
      (b) => open.has(b.id) && (b.type === "text" || b.type === "reasoning") && b.content !== "",
    );
  }
  return last.type === "toolCall" && (last.status === "running" || last.status === "pendingApproval");
}

/** A provider that withholds `id` until the last fragment is keyed as
 * `pending:{index}` while arguments stream — see
 * `ToolCallAccumulator::snapshots`. */
const PENDING_TOOL_ID = /^pending:(\d+)$/;

function findToolCallBlockIndex(
  blocks: MessageBlock[],
  call: { id: string; name?: string },
): number {
  const byId = blocks.findIndex((b) => b.type === "toolCall" && b.id === call.id);
  if (byId !== -1) return byId;
  if (PENDING_TOOL_ID.test(call.id)) return -1;
  const pending = blocks
    .map((b, i) => ({ b, i }))
    .filter(
      ({ b }) =>
        b.type === "toolCall" &&
        PENDING_TOOL_ID.test(b.id) &&
        (call.name === undefined || call.name === "" || b.name === call.name || b.name === ""),
    );
  return pending.length === 1 ? pending[0]!.i : -1;
}

/** A user turn stays a plain string — only an assistant turn's shape
 * changes, from flat `content` to an ordered `blocks` array. A
 * discriminated union on `role` (not an optional `content`/`blocks` pair)
 * so a user message can never carry `blocks` and an assistant message can
 * never carry `content` by construction. */
export type ChatMessage =
  | {
      id: string;
      role: "user";
      content: string;
      /** Set on the canned «Начать» user turn (`PLAN_EXECUTION_START_TEXT`)
       * so later Agent requests can drop the planning transcript from the
       * *wire* (not the UI) and start from this message. Persists with the
       * chat JSON blob — same as `isCompactionNotice` on assistant
       * messages, no backend column needed. */
      isPlanExecutionStart?: boolean;
    }
  | {
      id: string;
      role: "assistant";
      blocks: MessageBlock[];
      streaming?: boolean;
      failed?: boolean;
      /** Set alongside `failed` — the raw backend error text (network
       * failure, HTTP status, tool-budget exhaustion, ...) shown in the
       * error card rendered under this message, see
       * `AssistantConversation`. */
      errorMessage?: string;
      /** Set when the turn ended via a `{status: "cancelled"}` outcome (the
       * user clicked Stop — see `useLlmChat`'s `stopChat`) rather than the
       * model producing a final answer on its own. Mutually exclusive with
       * `failed`: a cancelled turn is a deliberate user action, not an
       * error. */
      cancelled?: boolean;
      /** Set when the provider cut this turn off because the response
       * budget ran out (`ChatStreamResult.truncated` — `finish_reason:
       * "length"`, i.e. the provider's configured `max_tokens`) instead of
       * the model finishing its own sentence. Not a failure and not a
       * cancellation: the text below it is real, it just stops early, and
       * without the note under it an unfinished answer is indistinguishable
       * from a complete one. */
      truncated?: boolean;
      /** Real token usage for this turn, when the provider reported one on
       * the final SSE chunk — only ever set on a completed assistant
       * message. */
      usage?: ChatUsage;
      /** Set only on a synthetic, display-only marker inserted by
       * `useLlmChat`'s history-compaction pass (see
       * `src/lib/contextCompaction.ts`) — a normal assistant message
       * (`blocks: [{type:"text", ...}]`, `streaming: false`) that renders as
       * a distinct pill rather than a chat bubble in
       * `AssistantConversation`, and that `realMessages`/`wireMessages`
       * construction must filter out before replaying history back to the
       * model (it describes the compaction event, it isn't part of the
       * conversation itself). Persists via the normal `chat_store` path —
       * no backend changes needed since it's just another JSON message. */
      isCompactionNotice?: boolean;
      /** Set on an `isCompactionNotice` message only while its pass is still
       * in flight — the marker is already in the transcript (so the wait for
       * the summarizer is visible as a card instead of an unexplained pause
       * before the reply starts), but `llmChatOnce` hasn't answered yet.
       * Absence means the pass settled, which is also how every notice
       * persisted before this field existed reads.
       *
       * Never reaches `chat_store`: the only save points are `runTurn`'s
       * `finally` (`onTurnSettled`) and the pause checkpoint, and the
       * compaction pass always resolves — success, empty summary, or the
       * `catch` — before either can run. */
      compactionRunning?: boolean;
      /** Which stream the most recent delta of this turn came from.
       *
       * Drives the "думает" shimmer, which cannot be read off the blocks
       * alone: a reasoning block stays *open* for the whole round on
       * purpose (some providers interleave `reasoning_content` with the
       * answer chunk by chunk, and closing it on the first content delta
       * shredded one answer into hundreds of blocks). Open is therefore not
       * the same as live — without this the thinking card kept shimmering,
       * and its timer kept running, for the entire time the answer streamed
       * underneath it. Only meaningful while `streaming` is true.
       */
      liveKind?: "text" | "reasoning";
      /** Set alongside `failed`/`errorMessage` when `isContextLengthError`
       * matches the raw error text — drives the "Сжать историю и
       * повторить" retry action in `AssistantConversation` (see
       * `useLlmChat`'s `retryWithCompaction`). */
      contextLengthExceeded?: boolean;
      /** Wall-clock duration of the whole turn — from `runTurn`'s start to
       * the moment `settleOutcome`/`settleError` marked this message
       * `streaming: false` — only ever set on a completed (success or
       * failed) assistant message, same as `usage` above. Not set to the
       * turn's true original duration when it was cold-resumed after an
       * app restart — there the timestamp only covers time since resume,
       * see `useLlmChat`'s cold-resume effect. */
      durationMs?: number;
    };

// ---- Pure block-transition rules -------------------------------------

/** Index of the block the current round is still streaming `type` into, or
 * `-1` when the round hasn't opened one yet.
 *
 * Scans backwards and stops at the first `toolCall`/`steer` block: those are
 * round boundaries (a tool call ends the prose that preceded it, a steer note
 * is injected as a fresh round starts), so anything before one belongs to an
 * already-closed round and must never be reopened. A block explicitly marked
 * `closed` (by `closeOpenBlocks`, on the backend's round-started report) is
 * skipped for the same reason — that is the boundary a round which simply
 * ended in prose has, and without it two rounds' answers were concatenated
 * mid-sentence.
 *
 * Crucially it does *not* stop at a block of the other streaming type. Some
 * providers interleave `reasoning_content` and `content` chunk by chunk
 * within one round instead of finishing all the thinking first — matching
 * only the trailing block there opened a brand-new block on every single
 * chunk, shredding one answer into hundreds of ~9-character blocks with a
 * thinking card flickering between each pair. Both streams now grow
 * in place, in whatever order they were opened. */
function findOpenBlockIndex(blocks: MessageBlock[], type: "text" | "reasoning"): number {
  for (let i = blocks.length - 1; i >= 0; i--) {
    const block = blocks[i]!;
    if (block.type === type) return block.closed ? -1 : i;
    if (block.type === "toolCall" || block.type === "steer") return -1;
  }
  return -1;
}

/** Ends the current round's prose: whatever text/reasoning block is still
 * open stops accepting deltas, so the next round opens its own.
 *
 * Called on `llm:round-started`. The other two boundaries (a tool call, a
 * user steer) are visible in the block list itself and need no marker; a
 * round that just ended in an answer and was followed by another round had
 * none at all, which is the bug this closes. Returns the input unchanged
 * when nothing was open, so React sees no new array. */
export function closeOpenBlocks(blocks: MessageBlock[]): MessageBlock[] {
  const openText = findOpenBlockIndex(blocks, "text");
  const openReasoning = findOpenBlockIndex(blocks, "reasoning");
  if (openText === -1 && openReasoning === -1) return blocks;
  return blocks.map((b, i) =>
    (i === openText || i === openReasoning) && (b.type === "text" || b.type === "reasoning")
      ? { ...b, closed: true as const }
      : b,
  );
}

function appendToOpenBlock(
  blocks: MessageBlock[],
  type: "text" | "reasoning",
  delta: string,
): MessageBlock[] {
  const index = findOpenBlockIndex(blocks, type);
  if (index !== -1) {
    return blocks.map((b, i) =>
      i === index && (b.type === "text" || b.type === "reasoning")
        ? { ...b, content: b.content + delta }
        : b,
    );
  }
  return [...blocks, { type, id: crypto.randomUUID(), content: delta }];
}

/** A `CHAT_STREAM_DELTA_EVENT` either extends this round's still-open text
 * block, or opens a fresh one if the round hasn't produced any prose yet
 * (see `findOpenBlockIndex` for what "this round" means). */
export function appendDeltaToBlocks(blocks: MessageBlock[], delta: string): MessageBlock[] {
  return appendToOpenBlock(blocks, "text", delta);
}

/** A `CHAT_STREAM_REASONING_EVENT` either extends this round's still-open
 * reasoning block, or opens a fresh one — same shape as
 * `appendDeltaToBlocks`, for a reasoning-capable model's "thinking" text
 * instead of its answer. A reasoning block is closed off by the next tool
 * call (or the end of the turn), not by the first `content` delta — a
 * provider that interleaves the two keeps filling this same block. */
export function appendReasoningDeltaToBlocks(blocks: MessageBlock[], delta: string): MessageBlock[] {
  return appendToOpenBlock(blocks, "reasoning", delta);
}

/** Repairs a persisted message whose rounds were split into many one-chunk
 * text/reasoning blocks, by re-running the block-transition rules above
 * over it: within one round (delimited by `toolCall`/`steer` blocks, see
 * `findOpenBlockIndex`) every text block is folded into the round's first
 * one and every reasoning block into the round's first one, in order and
 * with no separator — the split happened mid-word, so anything but plain
 * concatenation would corrupt the text.
 *
 * Applied when loading a chat from the store (`loadChatMessages`), so
 * conversations recorded before `appendDeltaToBlocks` tolerated interleaved
 * streams render as prose again instead of hundreds of stuttering cards —
 * and, just as importantly, replay into later requests as one coherent
 * message rather than as `flattenBlocksToText`'s `\n\n`-joined confetti.
 * Returns the input array unchanged when there was nothing to merge. */
export function mergeInterleavedStreamBlocks(blocks: MessageBlock[]): MessageBlock[] {
  const result: MessageBlock[] = [];
  let openText = -1;
  let openReasoning = -1;
  let merged = false;
  for (const block of blocks) {
    if (block.type === "toolCall" || block.type === "steer") {
      openText = -1;
      openReasoning = -1;
      result.push(block);
      continue;
    }
    // A closed block ended a round of its own: folding the next round's
    // answer into it would recreate, on load, exactly the concatenation
    // `closeOpenBlocks` exists to prevent.
    const openIndex = block.type === "text" ? openText : openReasoning;
    if (block.closed) {
      if (openIndex !== -1) {
        const open = result[openIndex] as TextBlock | ReasoningBlock;
        result[openIndex] = { ...open, content: open.content + block.content, closed: true };
        merged = true;
      } else {
        result.push(block);
      }
      if (block.type === "text") openText = -1;
      else openReasoning = -1;
      continue;
    }
    if (openIndex !== -1) {
      const open = result[openIndex] as TextBlock | ReasoningBlock;
      result[openIndex] = { ...open, content: open.content + block.content };
      merged = true;
      continue;
    }
    if (block.type === "text") openText = result.length;
    else openReasoning = result.length;
    result.push(block);
  }
  return merged ? result : blocks;
}

/** `mergeInterleavedStreamBlocks` over a whole loaded conversation. */
export function mergeInterleavedStreamBlocksInMessages(messages: ChatMessage[]): ChatMessage[] {
  return messages.map((m) => {
    if (m.role !== "assistant") return m;
    const blocks = mergeInterleavedStreamBlocks(m.blocks);
    return blocks === m.blocks ? m : { ...m, blocks };
  });
}

/** Ids of the blocks the model may still be writing into right now — this
 * round's open text and/or reasoning block. `AssistantConversation` uses it
 * for the streaming cursor and the shimmering thinking label:
 * position alone ("is it the last block") can't tell, since an interleaving
 * provider leaves the reasoning block sitting *above* a text block both are
 * still growing. Callers still gate on the message's own `streaming` flag —
 * this says which blocks are open, not whether a turn is in flight. */
export function openStreamingBlockIds(blocks: MessageBlock[]): Set<string> {
  const ids = new Set<string>();
  for (const type of ["text", "reasoning"] as const) {
    const index = findOpenBlockIndex(blocks, type);
    if (index !== -1) ids.add(blocks[index]!.id);
  }
  return ids;
}

/** A `TOOL_CALL_DELTA_EVENT` while the model is still writing a call's
 * arguments — same upsert-by-`id` as `appendToolCallBlock`, but it leaves
 * `status` alone on an existing block (a later pending-approval card, or
 * the eventual `TOOL_CALL_EVENT`, owns those transitions). A brand-new
 * id opens a `"running"` block so the transcript shows the call the moment
 * its `id`/`name` arrive, instead of sitting silent while a long
 * `visualize` source streams in. */
export function applyToolCallDelta(
  blocks: MessageBlock[],
  call: { id: string; name: string; argumentsJson: string },
): MessageBlock[] {
  const existingIndex = findToolCallBlockIndex(blocks, call);
  if (existingIndex !== -1) {
    return blocks.map((b, i) =>
      i === existingIndex && b.type === "toolCall"
        ? {
            ...b,
            id: call.id.startsWith("pending:") ? b.id : call.id,
            name: call.name !== "" ? call.name : b.name,
            argumentsJson: call.argumentsJson,
          }
        : b,
    );
  }
  return [
    ...blocks,
    {
      type: "toolCall",
      id: call.id,
      name: call.name,
      argumentsJson: call.argumentsJson,
      status: "running",
    },
  ];
}

/** A `TOOL_CALL_EVENT` normally pushes a brand-new `toolCall` block — this
 * is what closes off any open text block (the next delta, if any, sees a
 * trailing `toolCall` block and starts fresh per `appendDeltaToBlocks`).
 * The one exception: a call that was already shown — either as a
 * `"pendingApproval"` card (`appendPendingApprovalBlock`) or as a live
 * argument-stream block (`applyToolCallDelta`) — already has a block with
 * this exact `id`. This event is that same call now actually starting, not
 * a second one, so the existing block transitions in place (dropping
 * `deadlineAt`, the timer is moot once execution has begun) instead of
 * duplicating. Final `argumentsJson` overwrites whatever the stream had
 * accumulated, in case a delta was dropped. */
export function appendToolCallBlock(
  blocks: MessageBlock[],
  call: { id: string; name: string; argumentsJson: string; autoApproved?: boolean },
): MessageBlock[] {
  const existingIndex = findToolCallBlockIndex(blocks, call);
  if (existingIndex !== -1) {
    return blocks.map((b, i) =>
      i === existingIndex && b.type === "toolCall"
        ? {
            ...b,
            id: call.id,
            name: call.name !== "" ? call.name : b.name,
            argumentsJson: call.argumentsJson,
            status: "running",
            autoApproved: call.autoApproved,
            deadlineAt: undefined,
            approvalGroupId: undefined,
          }
        : b,
    );
  }
  return [
    ...blocks,
    {
      type: "toolCall",
      id: call.id,
      name: call.name,
      argumentsJson: call.argumentsJson,
      status: "running",
      autoApproved: call.autoApproved,
    },
  ];
}

export function appendSteerBlock(blocks: MessageBlock[], text: string): MessageBlock[] {
  return [...blocks, { type: "steer", id: crypto.randomUUID(), text }];
}

/** Shows a call awaiting user approval inline in the transcript, right
 * where it happened — a card with Approve/Deny actions and a countdown
 * strip toward `deadlineAt`, after which `useLlmChat` treats it as denied.
 * Always the trailing block for its call `id` until the real
 * `TOOL_CALL_EVENT` (via `appendToolCallBlock`) confirms execution
 * actually starting, whether the user decided manually or the timer ran
 * out. */
export function appendPendingApprovalBlock(
  blocks: MessageBlock[],
  call: {
    id: string;
    name: string;
    argumentsJson: string;
    deadlineAt?: number;
    approvalGroupId: string;
  },
): MessageBlock[] {
  const existingIndex = findToolCallBlockIndex(blocks, call);
  const next: ToolCallBlock = {
    type: "toolCall",
    id: call.id,
    name: call.name,
    argumentsJson: call.argumentsJson,
    status: "pendingApproval",
    deadlineAt: call.deadlineAt,
    approvalGroupId: call.approvalGroupId,
  };
  if (existingIndex !== -1) {
    return blocks.map((b, i) => (i === existingIndex ? next : b));
  }
  return [...blocks, next];
}

/** A `TOOL_RESULT_EVENT` finds the block by `id` (searching the whole
 * array, not just the tail — the matching `toolCall` block can be several
 * blocks back by the time this fires) and settles it to `done`/`error`. A
 * `result` that isn't `null` always means success, matching
 * `ToolResultEvent`'s "exactly one of result/error is Some"
 * contract on the Rust side. */
export function settleToolCallBlock(
  blocks: MessageBlock[],
  outcome: { id: string; result: ToolResult | null; error: string | null },
): MessageBlock[] {
  const existingIndex = findToolCallBlockIndex(blocks, { id: outcome.id });
  return blocks.map((b, i): MessageBlock => {
    if (b.type !== "toolCall" || (b.id !== outcome.id && i !== existingIndex)) return b;
    return outcome.result !== null
      ? { ...b, status: "done", result: outcome.result }
      : { ...b, status: "error", errorMessage: outcome.error ?? "Неизвестная ошибка" };
  });
}

/** `streamLlmChat()`'s resolved `text` is the authoritative full text of
 * the *final* round only (a safety net against a dropped delta) — by
 * construction the trailing block, if it's text, was built entirely from
 * that same final round's deltas (any earlier round's text was closed off
 * by an intervening `toolCall` block), so correcting only that one block is
 * exactly right: earlier text/tool-call blocks are untouched. If the
 * trailing block isn't text (the final round had a tool call as its very
 * last block, or there are no blocks yet) and `text` is non-empty, a new
 * trailing text block is appended instead of overwriting anything; an empty
 * `text` in that situation is a no-op. */
export function correctTrailingText(blocks: MessageBlock[], text: string): MessageBlock[] {
  const index = findOpenBlockIndex(blocks, "text");
  if (index !== -1) {
    return blocks.map((b, i) => (i === index && b.type === "text" ? { ...b, content: text } : b));
  }
  return text !== "" ? [...blocks, { type: "text", id: crypto.randomUUID(), content: text }] : blocks;
}

/** Overwrites one finished round's text block with the authoritative text
 * the backend accumulated for it (`llm:round-text`).
 *
 * `correctTrailingText` cannot do this job: it goes through
 * `findOpenBlockIndex`, which gives up the moment it meets a `toolCall`
 * scanning back — so the prose of a round that *ended* in a tool call, which
 * is most of them, was never reconciled with anything and a single dropped
 * delta truncated it permanently. This walks past the round's own tool calls
 * to reach the text block in front of them.
 *
 * It stops only at a `steer`, and at a `text` block already marked `closed`:
 * both mean the scan has left this round and reached an earlier one, whose
 * text belongs to a report that already happened. `closed` is only ever set
 * during a live turn by `closeOpenBlocks`, on the *next* round's
 * `llm:round-started` — which is why this event, fired as its own round
 * ends, always arrives while its block is still open.
 *
 * An empty `text` is a no-op rather than a blanking: a round with no prose
 * has no text block to correct, so an empty string here can only mean the
 * two sides disagree, and dropping visible content is the worse way to be
 * wrong. */
export function correctRoundText(blocks: MessageBlock[], text: string): MessageBlock[] {
  if (text === "") return blocks;
  // Where a brand-new block goes if the round has none: in front of the
  // tool calls it opened, never after them.
  let insertAt = blocks.length;
  for (let i = blocks.length - 1; i >= 0; i--) {
    const block = blocks[i]!;
    if (block.type === "steer") return blocks;
    if (block.type === "toolCall") {
      insertAt = i;
      continue;
    }
    // Never a boundary. Some providers interleave `reasoning_content` and
    // `content` chunk by chunk within one round, so a reasoning block can
    // sit either side of the round's prose — stopping here left the text
    // block unreached and appended the round's prose a second time after
    // its tool calls. `findOpenBlockIndex` skips them for the same reason.
    if (block.type === "reasoning") continue;
    if (block.type === "text") {
      if (block.closed) return blocks;
      return blocks.map((b, j) => (j === i && b.type === "text" ? { ...b, content: text } : b));
    }
  }
  // Every delta for this round was dropped — the text still belongs in the
  // transcript, in the place the round would have put it.
  const fresh: MessageBlock = { type: "text", id: crypto.randomUUID(), content: text };
  return [...blocks.slice(0, insertAt), fresh, ...blocks.slice(insertAt)];
}

/** Same safety-net role as `correctTrailingText`, for `reasoning` instead
 * of `text`. Only ever corrects an already-open trailing `reasoning` block
 * (the round ended before any `content` arrived) — unlike
 * `correctTrailingText`, it never appends a brand-new block when the
 * trailing one isn't a reasoning block: reasoning always precedes the
 * answer it led to, so a reasoning block can't correctly be tacked onto the
 * *end* of blocks that already moved on to text/tool-calls; if every
 * `CHAT_STREAM_REASONING_EVENT` for a round was somehow dropped (or the
 * provider interleaved its thinking with the answer, leaving a text block
 * sitting after it), that
 * round's reasoning is simply lost, same tradeoff this codebase already
 * accepts for earlier, non-trailing blocks elsewhere. */
export function correctTrailingReasoning(blocks: MessageBlock[], reasoning: string): MessageBlock[] {
  const index = findOpenBlockIndex(blocks, "reasoning");
  return index === -1
    ? blocks
    : blocks.map((b, i) => (i === index && b.type === "reasoning" ? { ...b, content: reasoning } : b));
}

/** Called when the overall `streamLlmChat()` promise rejects (hit
 * `MAX_TOOL_ITERATIONS`, a later round's HTTP call failed, `current_scope`
 * failed, or — the one case that can genuinely leave a call stuck — a panic
 * inside `execute_tool` on the Rust side, which skips its `TOOL_RESULT_EVENT`
 * entirely) — and, with `reason` set to something more specific, when a
 * `{status: "cancelled"}` outcome resolves instead of rejecting (see
 * `useLlmChat`'s `stopChat`): a pending-approval card auto-denied by
 * `stopChat` still never gets the `TOOL_CALL_EVENT`/`TOOL_RESULT_EVENT` pair
 * that would normally settle it, since `run_tool_loop` returns `Cancelled`
 * before ever reaching that round's calls once the flag is set. Any block
 * still `"running"` at that point will never receive its settling event
 * either, so it's swept to `"error"` here — otherwise its spinner (driven
 * by the block's own `status`, not the message's `streaming` flag) would
 * spin forever on an already-dead message. A `"pendingApproval"` block is
 * swept the same way — the request that would have resumed it (whether
 * decided by the user or by its own timeout) never went out. */
export function markRunningToolCallsAsInterrupted(
  blocks: MessageBlock[],
  reason = "Запрос прерван до получения результата",
): MessageBlock[] {
  return blocks.map((b): MessageBlock =>
    b.type === "toolCall" && (b.status === "running" || b.status === "pendingApproval")
      ? { ...b, status: "error", errorMessage: reason, deadlineAt: undefined, approvalGroupId: undefined }
      : b,
  );
}

/** Shared guard for every live-event handler: only the trailing message,
 * and only while it's still marked `streaming`, is ever mutated — a
 * straggler event arriving after that turn already finalized (or before
 * any turn has started) is a no-op.
 *
 * `liveKind` records which of the two streams this update came from, when
 * it came from one — see `ChatMessage["liveKind"]`. */
export function updateLastAssistantBlocks(
  messages: ChatMessage[],
  updater: (blocks: MessageBlock[]) => MessageBlock[],
  liveKind?: "text" | "reasoning",
): ChatMessage[] {
  const last = messages[messages.length - 1];
  if (!last || last.role !== "assistant" || !last.streaming) return messages;
  return [
    ...messages.slice(0, -1),
    { ...last, blocks: updater(last.blocks), ...(liveKind ? { liveKind } : {}) },
  ];
}

// ---- Grouping pending approvals for render -----------------------------

/** What `AssistantConversation` actually renders per entry: either one
 * ordinary block, a run of `"pendingApproval"` mutating/mode-switch calls
 * collapsed into one approval card, a run of pending `askUser` calls
 * collapsed into one ask card, or a run of pending `requestArtifact` calls
 * collapsed into one artifact card. */
export type RenderBlock =
  | { kind: "single"; block: MessageBlock }
  | { kind: "approvalGroup"; blocks: ToolCallBlock[] }
  | { kind: "askGroup"; blocks: ToolCallBlock[] }
  | { kind: "artifactGroup"; blocks: ToolCallBlock[] };

/** Walks a message's flat `blocks`, merging any run of adjacent
 * `"pendingApproval"` `toolCall` blocks that share one `approvalGroupId`
 * into a single group entry — `askGroup` when every block is `askUser`,
 * `artifactGroup` when every block is `requestArtifact`, otherwise
 * `approvalGroup` (including a run of length one). Every other block passes
 * through as `"single"` unchanged. */
export function groupBlocksForRender(blocks: MessageBlock[]): RenderBlock[] {
  const result: RenderBlock[] = [];
  for (const block of blocks) {
    const groupId =
      block.type === "toolCall" && block.status === "pendingApproval" ? block.approvalGroupId : undefined;
    const last = result[result.length - 1];
    if (
      groupId !== undefined &&
      last &&
      (last.kind === "approvalGroup" || last.kind === "askGroup" || last.kind === "artifactGroup") &&
      last.blocks[0]?.approvalGroupId === groupId
    ) {
      last.blocks.push(block as ToolCallBlock);
    } else if (groupId !== undefined) {
      const name = block.type === "toolCall" ? block.name : "";
      const kind =
        name === "askUser" ? "askGroup" : name === "requestArtifact" ? "artifactGroup" : "approvalGroup";
      result.push({ kind, blocks: [block as ToolCallBlock] });
    } else {
      result.push({ kind: "single", block });
    }
  }
  return result;
}

// ---- Flattening back to plain text (replay into future requests) ------

/** Joins every non-empty text block's content — both intermediate
 * commentary before a tool call and the final answer — plus applied user
 * steering notes, `\n\n`-separated; tool-call and reasoning blocks
 * contribute nothing. Deliberate small behavior change
 * from before blocks existed: previously *only* the final round's text
 * ever reached `ChatMessage.content` (intermediate-round prose was
 * streamed transiently then wiped, never persisted); now it's kept for
 * display, so it gets replayed too — what's replayed matches what's shown.
 *
 * Tool calls contribute nothing *here*; the file paths they touched are
 * added back separately by `chatMessageToPlainText` via `toolLedger`. */
export function flattenBlocksToText(blocks: MessageBlock[]): string {
  return blocks
    .flatMap((b) =>
      b.type === "text" && b.content !== ""
        ? [b.content]
        : b.type === "steer"
          ? [`${STEERING_PREFIX}${b.text}`]
          : [],
    )
    .join("\n\n");
}

/** Upper bound on paths in one turn's ledger. A 48-call research turn is
 * real (see `toolLedger`'s doc comment), and replaying every path from it
 * would cost more than the facts are worth — the most recent ones are the
 * ones a follow-up asks about. */
const TOOL_LEDGER_MAX_PATHS = 40;

/** Wire tools whose `path` argument names a file the assistant actually
 * saw the contents of. `check` is deliberately absent: it reports
 * diagnostics about a file rather than showing it. */
const READ_TOOLS = new Set(["readFile", "gitDiff", "gitBlame"]);

const WRITE_TOOLS = new Set(["writeFile", "editFile", "createDirectory"]);

const DELETE_TOOLS = new Set(["deleteFile", "deleteDirectory"]);

/** Reads `path`/`newPath` off a tool call's raw arguments JSON. Purely
 * cosmetic-grade parsing, like `describeToolActivity`'s: a call whose
 * arguments don't parse contributes nothing rather than throwing. */
export function toolCallPaths(block: ToolCallBlock): { path?: string; newPath?: string } {
  try {
    const parsed: unknown = JSON.parse(block.argumentsJson);
    if (!parsed || typeof parsed !== "object") return {};
    const args = parsed as Record<string, unknown>;
    return {
      path: typeof args.path === "string" ? args.path : undefined,
      newPath: typeof args.newPath === "string" ? args.newPath : undefined,
    };
  } catch {
    return {};
  }
}

/** A one-line record of which files this turn touched, for replay into
 * later turns.
 *
 * The problem it solves: tool calls and their results live only inside the
 * turn that made them (`services::llm_chat::run_tool_loop` keeps them in
 * its own `history`); cross-turn replay is `flattenBlocksToText`, which
 * keeps prose only. So a follow-up turn sees the assistant's *answer* but
 * has no record of the files behind it — and prose routinely shortens a
 * path to a basename. Observed consequence: a turn that had read
 * `.../thrift/services/AusnTransactionService.java` answered citing
 * `AusnTransactionService.java:41`; the next turn needed that file again,
 * reconstructed the directory from a neighbouring path in its own text,
 * and called `grep` on a path that does not exist.
 *
 * Paths, not results: a re-read is one cheap call, while an invented path
 * costs a failed call plus whatever the model does to recover. So this
 * replays only the identity of what was touched — never snippets, never
 * search hits (those are reproducible), never failed calls. Returns `""`
 * when the turn touched nothing. */
export function toolLedger(blocks: MessageBlock[]): string {
  const read: string[] = [];
  const written: string[] = [];
  const deleted: string[] = [];

  for (const block of blocks) {
    if (block.type !== "toolCall" || block.status !== "done") continue;
    const { path, newPath } = toolCallPaths(block);
    if (block.name === "move") {
      if (path && newPath) written.push(`${path} → ${newPath}`);
      continue;
    }
    if (!path) continue;
    if (READ_TOOLS.has(block.name)) read.push(path);
    else if (WRITE_TOOLS.has(block.name)) written.push(path);
    else if (DELETE_TOOLS.has(block.name)) deleted.push(path);
  }

  // Writes and deletes lead: they changed the project, so a later turn
  // reasoning about its current state needs them even more than reads —
  // and being first, they are never the entries the cap drops.
  const groups: (readonly [string, string[]])[] = [
    ["изменены", written],
    ["удалены", deleted],
    ["прочитаны", read],
  ];

  const sections: string[] = [];
  let budget = TOOL_LEDGER_MAX_PATHS;
  let omitted = 0;
  for (const [label, paths] of groups) {
    const unique = [...new Set(paths)];
    if (unique.length === 0) continue;
    // Keep the most recent entries of whatever no longer fits — a
    // follow-up question is about the end of the turn far more often than
    // its beginning.
    const kept = unique.slice(Math.max(0, unique.length - budget));
    omitted += unique.length - kept.length;
    budget -= kept.length;
    if (kept.length > 0) sections.push(`${label}: ${kept.join(", ")}`);
  }

  if (sections.length === 0) return "";
  const tail = omitted > 0 ? `; и ещё ${omitted} файл(ов)` : "";
  return `[Файлы, затронутые в этом ходе — ${sections.join("; ")}${tail}]`;
}

/** Whether this conversation's *most recent* search ran without the
 * semantic tier, i.e. the embedding API could not be reached (see
 * `services::ai_tools::tools::semantic_search`'s degraded path).
 *
 * Derived from the transcript rather than tracked as its own state: the
 * transcript already records what actually happened, so this can't drift,
 * needs no clearing on chat switch, and resolves itself the moment a later
 * search succeeds. Only the newest search counts — an outage earlier in a
 * long chat says nothing about now.
 *
 * Scans newest-first and stops at the first settled `semanticSearch`;
 * returns `false` for a conversation that has not searched at all. */
export function searchIsDegraded(messages: ChatMessage[]): boolean {
  for (let i = messages.length - 1; i >= 0; i--) {
    const message = messages[i];
    if (message === undefined || message.role !== "assistant") continue;
    for (let j = message.blocks.length - 1; j >= 0; j--) {
      const block = message.blocks[j];
      if (block === undefined || block.type !== "toolCall") continue;
      if (block.status !== "done" || block.result?.tool !== "semanticSearchResults") continue;
      return normalizeSemanticSearchResult(block.result.result).meta.degraded !== null;
    }
  }
  return false;
}

/** The plain-text projection of one `ChatMessage` regardless of role — what
 * both `contextTokens`'s `estimateTokenCount` sum and `sendMessage`'s
 * `wireMessages` replay need. An assistant turn carries its `toolLedger`
 * along with its prose, so the paths it worked on survive into the next
 * turn (the estimate counts them because the wire replay sends them). */
export function chatMessageToPlainText(message: ChatMessage): string {
  if (message.role === "user") return message.content;
  const text = flattenBlocksToText(message.blocks);
  const ledger = toolLedger(message.blocks);
  if (!ledger) return text;
  return text ? `${text}\n\n${ledger}` : ledger;
}

/** Fixed per-tool-call overhead the wire adds around what's counted
 * explicitly below: the assistant message's `tool_calls` entry (its `id`,
 * the `{"type":"function","function":{...}}` envelope) plus the `tool`
 * message that answers it (`role`, `tool_call_id`). Small, but there are
 * dozens of these in a research turn. */
const TOOL_CALL_WIRE_OVERHEAD_CHARS = 60;

/** Serialized length of a settled tool call's result, as the backend
 * writes it into the turn's `history` — `serde_json::to_string` over the
 * very same tagged structure this block carries (see
 * `services::llm_chat::run_tool_loop`), so stringifying it here measures
 * approximately the real payload rather than a proxy for it. Cosmetic-grade
 * parsing like `toolCallPaths`: a value that somehow won't serialize
 * contributes nothing rather than throwing inside a render. */
function toolResultWireLength(result: ToolResult): number {
  try {
    return JSON.stringify(result)?.length ?? 0;
  } catch {
    return 0;
  }
}

/** Token estimate for the context-usage ring only — *not* a wire
 * projection (that's `chatMessageToPlainText`, which both the replay in
 * `sendMessage` and the compaction trigger depend on; neither may use
 * this).
 *
 * The split it exists for: a tool call's arguments and its JSON result
 * live in the backend's per-turn `history` and are resent to the provider
 * on every subsequent round of *that* turn — thousands of tokens after a
 * `readFile`/`semanticSearch` — but they are gone by the next turn, where
 * `wireMessages` replays the turn as prose plus `toolLedger`. So only the
 * message still in flight is measured in full; a settled one is measured
 * as what will actually be resent. Counting tool payloads for every past
 * turn instead would pile up tens of thousands of tokens the request no
 * longer contains.
 *
 * Known, accepted imprecision (it's a progress indicator, and
 * `estimateTokenCount`'s ~4 chars/token is itself only a rule of thumb):
 * `listFiles` reaches the model as an ASCII tree (`render_file_tree`), not
 * as the JSON measured here; a denied `askUser`/`requestArtifact` collapses
 * to a short Russian line on the backend. Both over-count, mildly. */
export function estimateMessageContextTokens(message: ChatMessage): number {
  if (message.role === "user") return estimateTokenCount(message.content);
  if (!message.streaming) return estimateTokenCount(chatMessageToPlainText(message));

  let chars = flattenBlocksToText(message.blocks).length;
  for (const block of message.blocks) {
    if (block.type === "reasoning") {
      // Deliberately absent from `flattenBlocksToText` (it is never
      // replayed across turns) but present in the in-turn history.
      chars += block.content.length;
      continue;
    }
    if (block.type !== "toolCall") continue;
    chars += block.name.length + block.argumentsJson.length + TOOL_CALL_WIRE_OVERHEAD_CHARS;
    // A `running`/`pendingApproval` call has sent its arguments and
    // nothing else yet — there is no result in the history to count.
    if (block.status === "done" && block.result) chars += toolResultWireLength(block.result);
    else if (block.status === "error") chars += (block.errorMessage ?? "").length;
  }
  return estimateTokensFromChars(chars);
}
