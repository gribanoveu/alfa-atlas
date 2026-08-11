import { CONTEXT_COMPACTION_MIN_MESSAGES, CONTEXT_COMPACTION_TRIGGER_RATIO, describeToolResult } from "./assistantConfig";
import { chatMessageToPlainText, flattenBlocksToText, type ChatMessage, type ToolCallBlock } from "./chatBlocks";

/** What one proactive compaction pass leaves behind — cached by
 * `useLlmChat` (in a `useRef`, never persisted) so a later pass only has to
 * summarize the segment after `boundaryMessageId`, merging it into
 * `summaryText` rather than re-summarizing the whole conversation from
 * scratch every time. */
export type CompactionCache = {
  summaryText: string;
  boundaryMessageId: string;
};

export type CompactionPlan = {
  toSummarize: ChatMessage[];
  tail: ChatMessage[];
  newBoundaryId: string;
};

/** A synthetic, display-only marker `useLlmChat` inserts to make a
 * compaction pass visible in the transcript — never real conversation
 * content, must never be replayed back to the model. */
export function isCompactionNotice(m: ChatMessage): boolean {
  return m.role === "assistant" && Boolean(m.isCompactionNotice);
}

/** Real (non-notice) messages, in order — what compaction boundaries and
 * "keep last N" counts are computed against; notices are display-only and
 * never count toward them. */
export function realMessages(messages: ChatMessage[]): ChatMessage[] {
  return messages.filter((m) => !isCompactionNotice(m));
}

/** Whether a cached compaction still applies to `priorTurns` — false for a
 * cache carried over from a different conversation (defense in depth: in
 * practice `useLlmChat`'s `compactionCacheRef` already can't survive a chat
 * switch, since `AssistantConversation` remounts on `key={currentChatId}`,
 * but this makes the cache self-healing even if that guarantee ever
 * weakens) or one whose boundary message was somehow removed. Callers
 * should drop an invalid cache (treat it as `null`) rather than pass it to
 * `planCompaction`/use its `summaryText`. */
export function isCacheValid(cache: CompactionCache | null, priorTurns: ChatMessage[]): boolean {
  if (!cache) return false;
  return realMessages(priorTurns).some((m) => m.id === cache.boundaryMessageId);
}

/** Whether it's worth even attempting a compaction pass right now. `false`
 * when no `contextLimit` is configured (nothing to compare against — same
 * "informational only" caveat as the warning ring), or when the real
 * conversation is still too short for summarizing part of it to be worth an
 * extra LLM round trip. */
export function shouldCompact(estimatedTokens: number, contextLimit: number | null, priorTurns: ChatMessage[]): boolean {
  if (!contextLimit) return false;
  if (realMessages(priorTurns).length < CONTEXT_COMPACTION_MIN_MESSAGES) return false;
  return estimatedTokens >= contextLimit * CONTEXT_COMPACTION_TRIGGER_RATIO;
}

/** Computes the [summarize, keep-verbatim] split for one compaction pass.
 * `cache` (already confirmed valid via `isCacheValid`, or `null`) anchors
 * where the new segment starts — only messages after `boundaryMessageId`
 * are considered. Returns `null` when there's nothing worth folding away.
 *
 * Retention is recency-based with one relevance carve-out, not blind
 * "last N": messages within `keepLast` of the end always stay verbatim, and
 * scanning forward from the start of the new segment stops at the first
 * *older* message that mentions `activeFilePath` (the file the user
 * currently has open) — that message and everything after it stays in
 * `tail` for this pass. Stopping there (rather than splicing it out and
 * continuing to summarize past it) keeps `newBoundaryId` a clean, single
 * cutoff: "everything at or before this id is fully covered by
 * `summaryText`" stays true, so a later pass never has to reason about
 * gaps. In practice this means the assistant loses detail on a very old
 * message about the active file slightly later than it theoretically could
 * — an acceptable trade for a boundary model simple enough to trust. */
export function planCompaction(
  priorTurns: ChatMessage[],
  cache: CompactionCache | null,
  keepLast: number,
  activeFilePath: string | null,
): CompactionPlan | null {
  const real = realMessages(priorTurns);
  const boundaryIndex = cache ? real.findIndex((m) => m.id === cache.boundaryMessageId) : -1;
  const segment = real.slice(boundaryIndex + 1);
  const recencyFloorIndex = Math.max(0, segment.length - keepLast);

  const toSummarize: ChatMessage[] = [];
  for (let i = 0; i < recencyFloorIndex; i++) {
    const m = segment[i]!;
    const mentionsActiveFile = activeFilePath !== null && chatMessageToPlainText(m).includes(activeFilePath);
    if (mentionsActiveFile) break;
    toSummarize.push(m);
  }

  if (toSummarize.length === 0) return null;

  const newBoundaryId = toSummarize[toSummarize.length - 1]!.id;
  const tail = segment.slice(toSummarize.length);
  return { toSummarize, tail, newBoundaryId };
}

/** Compact, tool-call-aware serialization used only for the text fed into
 * the compaction summarizer — distinct from `chatMessageToPlainText`, which
 * `wireMessages`'s normal cross-turn replay uses and which already drops
 * tool-call blocks entirely (see `flattenBlocksToText`), keeping only final
 * text. That existing behavior already keeps raw tool *output* out of
 * normal replay, so this isn't a cost fix — it's a quality fix: without it,
 * the summarizer sees only final assistant prose and has no signal at all
 * about which files were actually touched, so a `FILES:` section would come
 * out empty or hallucinated. This adds that signal back, compactly: one
 * line per settled tool call, reusing the same one-line result formatter
 * the collapsed tool-call UI already shows — never the raw
 * `result`/`errorMessage` payload. */
export function describeMessageForCompaction(m: ChatMessage): string {
  if (m.role === "user") return `User: ${m.content}`;

  const lines: string[] = [];
  const text = flattenBlocksToText(m.blocks);
  if (text) lines.push(`Assistant: ${text}`);

  const toolLines = m.blocks
    .filter((b): b is ToolCallBlock => b.type === "toolCall" && (b.status === "done" || b.status === "error"))
    .map((b) => `  [tool] ${b.name} -> ${describeToolResult(b)}`);
  lines.push(...toolLines);

  return lines.join("\n");
}

/** The programmatic (no LLM involved — no token cost either way) pill text
 * shown for a compaction-notice message. `fromOrdinal`/`toOrdinal` are
 * 1-based positions within `realMessages(messages)` at the time of the
 * pass. */
export function formatCompactionNoticeText(fromOrdinal: number, toOrdinal: number): string {
  return fromOrdinal === toOrdinal
    ? `История сжата (сообщение ${fromOrdinal} свёрнуто в резюме)`
    : `История сжата (сообщения ${fromOrdinal}–${toOrdinal} свёрнуты в резюме)`;
}

const CONTEXT_LENGTH_ERROR_PATTERN =
  /context.length|context_length_exceeded|maximum context length|too many tokens|prompt is too long/i;

/** Best-effort detection of a provider "context too long" error from the
 * opaque error string `useLlmChat`'s catch block receives (see
 * `LlmError::Http` on the Rust side, which folds the raw HTTP status+body
 * into one string) — providers phrase this differently, so this is a
 * heuristic, not a reliable classification. Drives the reactive "Сжать
 * историю и повторить" retry action. */
export function isContextLengthError(message: string): boolean {
  return CONTEXT_LENGTH_ERROR_PATTERN.test(message);
}
