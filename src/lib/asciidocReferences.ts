/**
 * Finds `include::`/`image::`/`xref:` macro targets in a line of AsciiDoc
 * text — the raw-text counterpart to the equivalent tokenizer rule in
 * `monaco/asciidocLanguage.ts` (kept in sync with that macro vocabulary),
 * but scoped to just the three kinds the workspace index tracks and
 * structured to answer "what's under this column" rather than just
 * highlighting. Used by `useMonacoDefinitions.ts` for Ctrl+Click navigation.
 *
 * Pure and framework-agnostic on purpose (no `monaco` import) — same style
 * as `monaco/asciidocSymbols.ts`.
 */

export type MacroTargetKind = "include" | "image" | "xref";

export type MacroTarget = {
  kind: MacroTargetKind;
  /** The path portion of the target, with any `#anchor` suffix removed. */
  target: string;
  anchor: string | null;
  /** 1-based Monaco column where `target` starts (inclusive). */
  startColumn: number;
  /** 1-based Monaco column where `target` ends (exclusive). */
  endColumn: number;
};

const MACRO_RE = /\b(image|include|xref):{1,2}([^\s[]*)/g;

/** Returns the macro target under `column` on `lineText`, or `null`. */
export function findMacroTargetAt(lineText: string, column: number): MacroTarget | null {
  MACRO_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = MACRO_RE.exec(lineText)) !== null) {
    const kind = match[1] as MacroTargetKind;
    const raw = match[2];
    if (!raw) continue;

    // `raw` is always the suffix of the full match, so this works
    // regardless of whether the macro used one or two colons.
    const startIndex = match.index + match[0].length - raw.length;
    const endIndex = startIndex + raw.length;
    const startColumn = startIndex + 1;
    const endColumn = endIndex + 1;
    if (column < startColumn || column > endColumn) continue;

    const hashIdx = raw.indexOf("#");
    const target = hashIdx >= 0 ? raw.slice(0, hashIdx) : raw;
    const anchor = hashIdx >= 0 ? raw.slice(hashIdx + 1) : null;
    if (!target) continue;

    return { kind, target, anchor, startColumn, endColumn };
  }
  return null;
}
