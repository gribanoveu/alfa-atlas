/** Narrows an unknown caught value to a displayable message.
 *
 * `catch` gives `unknown`, and what actually arrives here is almost always
 * one of two things: an `Error` thrown by frontend code, or the plain string
 * a rejected `invoke()` produces (every Tauri command in this app returns
 * `Result<_, String>`, so its rejection value is that string, not an
 * `Error`). `String(e)` covers both the latter and anything unexpected.
 *
 * Deliberately not localized: the caller decides whether to show this raw or
 * put it through something like `friendlyGitError` first. */
export function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
