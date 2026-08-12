/**
 * Finds `include::`/`image::`/`xref:` macro targets in a line of AsciiDoc
 * text — the raw-text counterpart to the equivalent tokenizer rule in
 * `monaco/asciidocLanguage.ts` (kept in sync with that macro vocabulary),
 * but scoped to just the three kinds the workspace index tracks and
 * structured to answer "what's under this column" rather than just
 * highlighting. Used by `useMonacoDefinitions.ts` for Ctrl+Click navigation
 * and `useMonacoIncludeGutter.ts` for the click-to-navigate gutter icon.
 *
 * Pure and framework-agnostic on purpose (no `monaco` import) — same style
 * as `monaco/asciidocSymbols.ts`.
 */

import { findDocument, getDocumentById } from "./workspaceIndex";
import { resolveRelativeToDocument, toDocsRelativePath, toRepoRelativePath } from "./paths";

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

function parseMacroMatch(match: RegExpExecArray): MacroTarget | null {
  const kind = match[1] as MacroTargetKind;
  const raw = match[2];
  if (!raw) return null;

  // `raw` is always the suffix of the full match, so this works regardless
  // of whether the macro used one or two colons.
  const startIndex = match.index + match[0].length - raw.length;
  const endIndex = startIndex + raw.length;

  const hashIdx = raw.indexOf("#");
  const target = hashIdx >= 0 ? raw.slice(0, hashIdx) : raw;
  const anchor = hashIdx >= 0 ? raw.slice(hashIdx + 1) : null;
  if (!target) return null;

  return { kind, target, anchor, startColumn: startIndex + 1, endColumn: endIndex + 1 };
}

/** Returns the macro target under `column` on `lineText`, or `null`. */
export function findMacroTargetAt(lineText: string, column: number): MacroTarget | null {
  MACRO_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = MACRO_RE.exec(lineText)) !== null) {
    const parsed = parseMacroMatch(match);
    if (!parsed) continue;
    if (column < parsed.startColumn || column > parsed.endColumn) continue;
    return parsed;
  }
  return null;
}

/**
 * Every macro target on `lineText`, in order — the whole-line counterpart to
 * `findMacroTargetAt`'s "what's under this column". Used by
 * `useMonacoIncludeGutter.ts` to decide which lines get a navigation icon
 * (a line can carry more than one macro, e.g. two `xref:`s — the gutter
 * icon navigates to whichever of them resolves first).
 */
export function findAllMacroTargets(lineText: string): MacroTarget[] {
  MACRO_RE.lastIndex = 0;
  const targets: MacroTarget[] = [];
  let match: RegExpExecArray | null;
  while ((match = MACRO_RE.exec(lineText)) !== null) {
    const parsed = parseMacroMatch(match);
    if (parsed) targets.push(parsed);
  }
  return targets;
}

/**
 * Resolves a macro target to the docs-root-relative path of the document it
 * points at, or `null` if nothing in the workspace index matches — same
 * two-step resolution `useMonacoDefinitions.ts`'s Ctrl+Click definition
 * provider uses (dir-relative lookup first, falling back to a by-filename
 * index search for a repo-relative or otherwise-not-dir-relative target),
 * now shared with `useMonacoIncludeGutter.ts` so both features agree on
 * what a macro target resolves to.
 */
export async function resolveMacroTargetDocsRelative(
  macroTarget: MacroTarget,
  sourceDocsRelative: string,
  repoRoot: string,
  docsRoot: string,
): Promise<string | null> {
  const naiveDocsRelative = resolveRelativeToDocument(macroTarget.target, sourceDocsRelative);
  const naiveRepoRelative = toRepoRelativePath(naiveDocsRelative, repoRoot, docsRoot);
  const naiveDoc = await getDocumentById(naiveRepoRelative).catch(() => null);
  if (naiveDoc) return naiveDocsRelative;

  // Dir-relative resolution didn't hit a real document — fall back to a
  // by-filename index lookup (also covers targets that were inserted as
  // repo-relative paths by older autocomplete).
  const basename = macroTarget.target.split("/").pop() || macroTarget.target;
  const matches = await findDocument(basename).catch(() => []);
  if (matches.length > 0) {
    return toDocsRelativePath(matches[0].relativePath, repoRoot, docsRoot);
  }
  return null;
}
