import { isContextLengthError } from "../contextCompaction";

/**
 * Closed set of assistant-failure classes. Deliberately small: the point
 * is to answer "what breaks turns" with a handful of comparable buckets,
 * not to reproduce provider error text — which is free-form, can contain
 * a prompt excerpt or an internal URL, and must never be sent.
 */
export type LlmErrorClass =
  | "rateLimit"
  | "contextLength"
  | "auth"
  | "network"
  | "cancelled"
  | "provider"
  | "unknown";

/**
 * Best-effort bucketing of a provider error message. Order matters: the
 * more specific patterns are tested before the generic HTTP-status one,
 * since a rate limit is also a 429 and an auth failure also a 401.
 *
 * Like `isContextLengthError`, this is a heuristic over text a provider
 * chose to return, not a reliable classification — `unknown` is a normal
 * outcome, not a bug.
 */
export function classifyLlmError(message: string): LlmErrorClass {
  const text = message.toLowerCase();

  if (isContextLengthError(message)) return "contextLength";
  if (/rate.?limit|too many requests|\b429\b|quota/.test(text)) return "rateLimit";
  if (/\b401\b|\b403\b|unauthorized|forbidden|invalid api key|api key/.test(text)) {
    return "auth";
  }
  if (/abort|cancel|отмен|останов/.test(text)) return "cancelled";
  if (
    /network|timeout|timed out|connection|connect|dns|unreachable|tls|certificate|socket/.test(
      text,
    )
  ) {
    return "network";
  }
  if (/\b(4\d\d|5\d\d)\b|http status|server error|bad gateway/.test(text)) {
    return "provider";
  }
  return "unknown";
}
