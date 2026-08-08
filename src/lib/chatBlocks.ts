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

export type ToolCallStatus = "running" | "done" | "error";

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
      /** Real token usage for this turn, when the provider reported one on
       * the final SSE chunk — only ever set on a completed assistant
       * message. */
      usage?: ChatUsage;
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

/** A `TOOL_CALL_EVENT` always pushes a brand-new `toolCall` block — this is
 * what closes off any open text block (the next delta, if any, sees a
 * trailing `toolCall` block and starts fresh per `appendDeltaToBlocks`). */
export function appendToolCallBlock(
  blocks: MessageBlock[],
  call: { id: string; name: string; argumentsJson: string },
): MessageBlock[] {
  return [
    ...blocks,
    { type: "toolCall", id: call.id, name: call.name, argumentsJson: call.argumentsJson, status: "running" },
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
 * entirely). Any block still `"running"` at that point will never receive
 * its settling event, so it's swept to `"error"` here — otherwise its
 * spinner (driven by the block's own `status`, not the message's `streaming`
 * flag) would spin forever on an already-dead message. */
export function markRunningToolCallsAsInterrupted(blocks: MessageBlock[]): MessageBlock[] {
  return blocks.map((b): MessageBlock =>
    b.type === "toolCall" && b.status === "running"
      ? { ...b, status: "error", errorMessage: "Запрос прерван до получения результата" }
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
