/** Rough token-count estimate for a chat message, used only to drive the
 * context-usage progress bar in the assistant panel — not an authoritative
 * accounting (no per-model tokenizer is available client-side, and none of
 * the providers this client talks to return usage counts today). ~4
 * characters per token is the common English rule-of-thumb quoted by
 * OpenAI's own docs; Cyrillic text likely tokenizes more densely than
 * that (fewer characters per token), so this tends to *underestimate* for
 * Russian-heavy conversations — acceptable for a progress indicator, not
 * for anything that needs to be exact. */
const CHARS_PER_TOKEN = 4;

export function estimateTokenCount(text: string): number {
  if (!text) return 0;
  return Math.ceil(text.length / CHARS_PER_TOKEN);
}
