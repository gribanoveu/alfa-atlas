import type { FileEdit } from "./aiTools";

/** Mirrors `services::ai_tools::apply_edits_exact`'s result shape
 * (`src-tauri/src/services/ai_tools/tools/edit_file.rs`) — used client-side purely to
 * render a pre-approval diff preview for a pending `editFile` call
 * (`EditFileDiffReview.tsx`), never sent anywhere. The actual edit is still
 * applied server-side by the real Rust implementation; this only has to
 * agree with it closely enough for the preview to be trustworthy. */
export type EditApplyResult =
  | { ok: true; content: string }
  | { ok: false; reason: "notFound"; old: string }
  | { ok: false; reason: "ambiguous"; old: string; count: number }
  | { ok: false; reason: "overlap" };

/** Applies every edit in `edits` to `content`, exact-match only — no
 * fast-apply/LLM fallback, which only exists server-side and can't be
 * replicated in a preview. Mirrors
 * `apply_edits_exact`/`exact_match_ranges`/`find_unique_exact_match`
 * exactly: each edit's `old` is looked up in `content` as given (never
 * against another edit's output), so edits are independent of each other
 * and of their own order. `old` missing entirely, appearing more than
 * once, or two edits' matched regions overlapping all reject the whole
 * call — same three failure reasons, first failing edit (in array order)
 * wins. JS string indices (UTF-16 code units) stand in for Rust's byte
 * offsets — irrelevant here since offsets only ever slice this same
 * string, never compared against the backend's. */
export function applyEditsExact(content: string, edits: FileEdit[]): EditApplyResult {
  const ranges: { start: number; end: number; new: string }[] = [];

  for (const edit of edits) {
    const match = findUniqueExactMatch(content, edit.old);
    if (!match.ok) return match.result;
    ranges.push({ start: match.start, end: match.end, new: edit.new });
  }

  ranges.sort((a, b) => a.start - b.start);
  for (let i = 1; i < ranges.length; i++) {
    if (ranges[i].start < ranges[i - 1].end) {
      return { ok: false, reason: "overlap" };
    }
  }

  let result = "";
  let cursor = 0;
  for (const { start, end, new: replacement } of ranges) {
    result += content.slice(cursor, start) + replacement;
    cursor = end;
  }
  result += content.slice(cursor);
  return { ok: true, content: result };
}

type MatchResult =
  | { ok: true; start: number; end: number }
  | { ok: false; result: Extract<EditApplyResult, { ok: false }> };

/** Finds `old`'s single occurrence in `content` — the same check
 * `apply_edits_exact` runs per edit on the Rust side. */
function findUniqueExactMatch(content: string, old: string): MatchResult {
  const start = content.indexOf(old);
  if (start === -1) {
    return { ok: false, result: { ok: false, reason: "notFound", old } };
  }
  let count = 1;
  let next = content.indexOf(old, start + 1);
  while (next !== -1) {
    count += 1;
    next = content.indexOf(old, next + 1);
  }
  if (count > 1) {
    return { ok: false, result: { ok: false, reason: "ambiguous", old, count } };
  }
  return { ok: true, start, end: start + old.length };
}
