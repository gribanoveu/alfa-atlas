import type { ToolResult } from "./aiTools";
import type { ChatUsage } from "./llm";

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

export type MessageBlock = TextBlock | ToolCallBlock;

/** A user turn stays a plain string — only an assistant turn's shape
 * changes, from flat `content` to an ordered `blocks` array. A
 * discriminated union on `role` (not an optional `content`/`blocks` pair)
 * so a user message can never carry `blocks` and an assistant message can
 * never carry `content` by construction. */
export type ChatMessage =
  | { id: string; role: "user"; content: string }
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
      /** Set alongside `failed`/`errorMessage` when `isContextLengthError`
       * matches the raw error text — drives the "Сжать историю и
       * повторить" retry action in `AssistantConversation` (see
       * `useLlmChat`'s `retryWithCompaction`). */
      contextLengthExceeded?: boolean;
    };

// ---- Pure block-transition rules -------------------------------------

/** A `CHAT_STREAM_DELTA_EVENT` either extends the still-open trailing text
 * block, or opens a fresh one if the message has no blocks yet or its last
 * block is a tool call (closed off by definition — a tool call always ends
 * whatever text preceded it). */
export function appendDeltaToBlocks(blocks: MessageBlock[], delta: string): MessageBlock[] {
  const last = blocks[blocks.length - 1];
  if (last && last.type === "text") {
    return [...blocks.slice(0, -1), { ...last, content: last.content + delta }];
  }
  return [...blocks, { type: "text", id: crypto.randomUUID(), content: delta }];
}

/** A `TOOL_CALL_EVENT` normally pushes a brand-new `toolCall` block — this
 * is what closes off any open text block (the next delta, if any, sees a
 * trailing `toolCall` block and starts fresh per `appendDeltaToBlocks`).
 * The one exception: a call that was shown inline as a `"pendingApproval"`
 * card (`appendPendingApprovalBlock`) already has a block with this exact
 * `id` — the round paused to show it before executing anything, and this
 * event is that same call now actually starting, not a second one, so the
 * existing block transitions in place (dropping `deadlineAt`, the timer is
 * moot once execution has begun) instead of duplicating. */
export function appendToolCallBlock(
  blocks: MessageBlock[],
  call: { id: string; name: string; argumentsJson: string; autoApproved?: boolean },
): MessageBlock[] {
  const existingIndex = blocks.findIndex((b) => b.type === "toolCall" && b.id === call.id);
  if (existingIndex !== -1) {
    return blocks.map((b, i) =>
      i === existingIndex && b.type === "toolCall"
        ? { ...b, status: "running", autoApproved: call.autoApproved, deadlineAt: undefined, approvalGroupId: undefined }
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

/** Shows a call awaiting user approval inline in the transcript, right
 * where it happened — a card with Approve/Deny actions and a countdown
 * strip toward `deadlineAt`, after which `useLlmChat` treats it as denied.
 * Always the trailing block for its call `id` until the real
 * `TOOL_CALL_EVENT` (via `appendToolCallBlock`) confirms execution
 * actually starting, whether the user decided manually or the timer ran
 * out. */
export function appendPendingApprovalBlock(
  blocks: MessageBlock[],
  call: { id: string; name: string; argumentsJson: string; deadlineAt: number; approvalGroupId: string },
): MessageBlock[] {
  return [
    ...blocks,
    {
      type: "toolCall",
      id: call.id,
      name: call.name,
      argumentsJson: call.argumentsJson,
      status: "pendingApproval",
      deadlineAt: call.deadlineAt,
      approvalGroupId: call.approvalGroupId,
    },
  ];
}

/** A `TOOL_RESULT_EVENT` finds the block by `id` (searching the whole
 * array, not just the tail — the matching `toolCall` block can be several
 * blocks back by the time this fires) and settles it to `done`/`error`. A
 * `result` that isn't `null` always means success, matching
 * `ToolResultEventPayload`'s "exactly one of result/error is Some"
 * contract on the Rust side. */
export function settleToolCallBlock(
  blocks: MessageBlock[],
  outcome: { id: string; result: ToolResult | null; error: string | null },
): MessageBlock[] {
  return blocks.map((b): MessageBlock => {
    if (b.type !== "toolCall" || b.id !== outcome.id) return b;
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
  const last = blocks[blocks.length - 1];
  if (last && last.type === "text") {
    return [...blocks.slice(0, -1), { ...last, content: text }];
  }
  return text !== "" ? [...blocks, { type: "text", id: crypto.randomUUID(), content: text }] : blocks;
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
 * any turn has started) is a no-op. */
export function updateLastAssistantBlocks(
  messages: ChatMessage[],
  updater: (blocks: MessageBlock[]) => MessageBlock[],
): ChatMessage[] {
  const last = messages[messages.length - 1];
  if (!last || last.role !== "assistant" || !last.streaming) return messages;
  return [...messages.slice(0, -1), { ...last, blocks: updater(last.blocks) }];
}

// ---- Grouping pending approvals for render -----------------------------

/** What `AssistantConversation` actually renders per entry: either one
 * ordinary block, or a run of `"pendingApproval"` `toolCall` blocks from the
 * same paused round collapsed into a single combined approval card. */
export type RenderBlock =
  | { kind: "single"; block: MessageBlock }
  | { kind: "approvalGroup"; blocks: ToolCallBlock[] };

/** Walks a message's flat `blocks`, merging any run of adjacent
 * `"pendingApproval"` `toolCall` blocks that share one `approvalGroupId`
 * into a single `"approvalGroup"` entry — including a run of length one, so
 * `AssistantToolApprovalGroup` is the only card component regardless of how
 * many calls a round paused on. Every other block passes through as
 * `"single"` unchanged. Adjacency always holds because
 * `appendPendingApprovalBlock` only ever appends a whole round's blocks
 * together in one `setMessages` call (see `useLlmChat`'s
 * `collectDecisions`). */
export function groupBlocksForRender(blocks: MessageBlock[]): RenderBlock[] {
  const result: RenderBlock[] = [];
  for (const block of blocks) {
    const groupId =
      block.type === "toolCall" && block.status === "pendingApproval" ? block.approvalGroupId : undefined;
    const last = result[result.length - 1];
    if (groupId !== undefined && last?.kind === "approvalGroup" && last.blocks[0]?.approvalGroupId === groupId) {
      last.blocks.push(block as ToolCallBlock);
    } else if (groupId !== undefined) {
      result.push({ kind: "approvalGroup", blocks: [block as ToolCallBlock] });
    } else {
      result.push({ kind: "single", block });
    }
  }
  return result;
}

// ---- Flattening back to plain text (replay into future requests) ------

/** Joins every non-empty text block's content — both intermediate
 * commentary before a tool call and the final answer — `\n\n`-separated;
 * tool-call blocks contribute nothing. Deliberate small behavior change
 * from before blocks existed: previously *only* the final round's text
 * ever reached `ChatMessage.content` (intermediate-round prose was
 * streamed transiently then wiped, never persisted); now it's kept for
 * display, so it gets replayed too — what's replayed matches what's shown. */
export function flattenBlocksToText(blocks: MessageBlock[]): string {
  return blocks
    .filter((b): b is TextBlock => b.type === "text" && b.content !== "")
    .map((b) => b.content)
    .join("\n\n");
}

/** The plain-text projection of one `ChatMessage` regardless of role — what
 * both `contextTokens`'s `estimateTokenCount` sum and `sendMessage`'s
 * `wireMessages` replay need. */
export function chatMessageToPlainText(message: ChatMessage): string {
  return message.role === "user" ? message.content : flattenBlocksToText(message.blocks);
}
