/**
 * Detects an in-progress AsciiDoc block macro whose path is complete enough
 * to close with `[]` (`include::`, `image::`, `xref:`). Used by the Monaco
 * on-type hook so Space/Enter after `include::request.adoc` yields
 * `include::request.adoc[]`.
 */

const UNCLOSED_MACRO_RE = /(?:include::|image::|xref:)([^\s\[]+)$/;

/** True when `prefix` ends with a bare `include::`/`image::`/`xref:` target. */
export function isUnclosedMacroPrefix(prefix: string): boolean {
  const match = UNCLOSED_MACRO_RE.exec(prefix);
  if (!match) return false;
  const target = match[1];
  return target.length > 0 && !target.endsWith("/") && !target.endsWith("#");
}

/**
 * Whether typing a terminator (Space/Enter) after `prefix` should insert `[]`
 * before that terminator. `suffix` is whatever follows the terminator (rest
 * of the line, or the next line after Enter) — skipped when brackets are
 * already there (xref snippet `path$0[]`).
 */
export function shouldInsertMacroBrackets(prefix: string, suffix: string): boolean {
  if (suffix.trimStart().startsWith("[")) return false;
  return isUnclosedMacroPrefix(prefix);
}
