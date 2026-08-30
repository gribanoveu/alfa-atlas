/** Rough token-count estimate for a chat message, used only to drive the
 * context-usage progress bar in the assistant panel — not an authoritative
 * accounting (no per-model tokenizer is available client-side, and not
 * every provider this client talks to returns usage counts; the ones that
 * do already anchor the count, see `contextTokens` in `useLlmChat`). ~4
 * characters per token is the common English rule-of-thumb quoted by
 * OpenAI's own docs; Cyrillic text likely tokenizes more densely than
 * that (fewer characters per token), so this tends to *underestimate* for
 * Russian-heavy conversations — acceptable for a progress indicator, not
 * for anything that needs to be exact. */
const CHARS_PER_TOKEN = 4;

/** The same estimate straight from a character count, for callers that
 * accumulate lengths across many strings (see
 * `estimateMessageContextTokens`) rather than joining them into one — the
 * join would allocate a copy of every tool result in the turn just to
 * measure it. */
export function estimateTokensFromChars(chars: number): number {
  if (chars <= 0) return 0;
  return Math.ceil(chars / CHARS_PER_TOKEN);
}

export function estimateTokenCount(text: string): number {
  if (!text) return 0;
  return estimateTokensFromChars(text.length);
}
