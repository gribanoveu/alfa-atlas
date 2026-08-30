import { normalizeSemanticSearchResult, type ToolResult } from "./aiTools";
import { STEERING_PREFIX, type ChatUsage } from "./llm";

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

/** A reasoning-capable model's "thinking" text (`reasoning_content` on the
 * wire), streamed ahead of the model's actual answer. Always closed off by
 * whatever follows it — the first `content` delta after some
 * `reasoning_content` opens a fresh `TextBlock` rather than extending this
 * one (see `appendDeltaToBlocks`), so "is this block still growing" is never
 * stored on the block itself: it's derived the same way a trailing
 * `TextBlock`'s own streaming cursor is, from the block's position (last in
 * the array) plus the message's own `streaming` flag — see
 * `AssistantConversation`. */
export type ReasoningBlock = {
  type: "reasoning";
  id: string;
  content: string;
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

/** True when the trailing block already tells the user the model is busy
 * (growing prose, visible reasoning, or a tool still in flight). False
 * after a settled tool call / empty transcript — that's when the chat
 * must show "Модель думает…", for every provider, not only those that
 * send `reasoning_content`. */
export function lastBlockShowsLiveProgress(blocks: MessageBlock[]): boolean {
  const last = blocks[blocks.length - 1];
  if (!last) return false;
  switch (last.type) {
    case "text":
    case "reasoning":
      return last.content !== "";
    case "toolCall":
      return last.status === "running" || last.status === "pendingApproval";
    case "steer":
      return false;
  }
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

/** A `CHAT_STREAM_REASONING_EVENT` either extends the still-open trailing
 * reasoning block, or opens a fresh one — same shape as `appendDeltaToBlocks`,
 * for a reasoning-capable model's "thinking" text instead of its answer.
 * Once a `content` delta arrives, `appendDeltaToBlocks` won't find a
 * trailing `"text"` block here and opens a new one instead of extending
 * this one — that's what closes a reasoning block off, no explicit
 * transition needed on this side. */
export function appendReasoningDeltaToBlocks(blocks: MessageBlock[], delta: string): MessageBlock[] {
  const last = blocks[blocks.length - 1];
  if (last && last.type === "reasoning") {
    return [...blocks.slice(0, -1), { ...last, content: last.content + delta }];
  }
  return [...blocks, { type: "reasoning", id: crypto.randomUUID(), content: delta }];
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
  const last = blocks[blocks.length - 1];
  if (last && last.type === "text") {
    return [...blocks.slice(0, -1), { ...last, content: text }];
  }
  return text !== "" ? [...blocks, { type: "text", id: crypto.randomUUID(), content: text }] : blocks;
}

/** Same safety-net role as `correctTrailingText`, for `reasoning` instead
 * of `text`. Only ever corrects an already-open trailing `reasoning` block
 * (the round ended before any `content` arrived) — unlike
 * `correctTrailingText`, it never appends a brand-new block when the
 * trailing one isn't a reasoning block: reasoning always precedes the
 * answer it led to, so a reasoning block can't correctly be tacked onto the
 * *end* of blocks that already moved on to text/tool-calls; if every
 * `CHAT_STREAM_REASONING_EVENT` for a round was somehow dropped, that
 * round's reasoning is simply lost, same tradeoff this codebase already
 * accepts for earlier, non-trailing blocks elsewhere. */
export function correctTrailingReasoning(blocks: MessageBlock[], reasoning: string): MessageBlock[] {
  const last = blocks[blocks.length - 1];
  return last && last.type === "reasoning" ? [...blocks.slice(0, -1), { ...last, content: reasoning }] : blocks;
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
