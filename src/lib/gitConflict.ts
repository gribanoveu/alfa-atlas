export type ConflictBlock = {
  /** 1-based line number of the `<<<<<<<` marker. */
  startLine: number;
  /** 1-based line number of the `=======` marker. */
  midLine: number;
  /** 1-based line number of the `>>>>>>>` marker. */
  endLine: number;
  /** Lines between `<<<<<<<` and `=======`, joined with `\n` (no trailing newline). */
  ours: string;
  /** Lines between `=======` and `>>>>>>>`, joined with `\n` (no trailing newline). */
  theirs: string;
};

/**
 * Parses `<<<<<<< / ======= / >>>>>>>` conflict markers out of file content
 * written by a failed merge checkout. Stops at the first unterminated
 * marker (missing `=======` or `>>>>>>>`) rather than guessing.
 */
export function parseConflictBlocks(text: string): ConflictBlock[] {
  const lines = text.split("\n");
  const blocks: ConflictBlock[] = [];
  let i = 0;

  while (i < lines.length) {
    if (!lines[i].startsWith("<<<<<<<")) {
      i++;
      continue;
    }

    const startIdx = i;
    let midIdx = -1;
    let endIdx = -1;
    for (let j = startIdx + 1; j < lines.length; j++) {
      if (midIdx === -1 && lines[j] === "=======") {
        midIdx = j;
      } else if (midIdx !== -1 && lines[j].startsWith(">>>>>>>")) {
        endIdx = j;
        break;
      }
    }
    if (midIdx === -1 || endIdx === -1) break;

    blocks.push({
      startLine: startIdx + 1,
      midLine: midIdx + 1,
      endLine: endIdx + 1,
      ours: lines.slice(startIdx + 1, midIdx).join("\n"),
      theirs: lines.slice(midIdx + 1, endIdx).join("\n"),
    });
    i = endIdx + 1;
  }

  return blocks;
}

/** Number of unresolved conflict blocks remaining in `text`. */
export function countConflictBlocks(text: string): number {
  return parseConflictBlocks(text).length;
}

export type ConflictSegment =
  | { type: "context"; text: string }
  | { type: "conflict"; block: ConflictBlock };

/**
 * Splits `text` into an ordered list of plain-context runs and conflict
 * blocks, so a UI can render the two very differently (e.g. plain text vs.
 * a "pick a version" card) without the reader ever seeing raw `<<<<<<<`
 * marker syntax.
 */
export function splitConflictSegments(text: string): ConflictSegment[] {
  const lines = text.split("\n");
  const blocks = parseConflictBlocks(text);
  const segments: ConflictSegment[] = [];
  let cursor = 0;

  for (const block of blocks) {
    const contextLines = lines.slice(cursor, block.startLine - 1);
    if (contextLines.length > 0) {
      segments.push({ type: "context", text: contextLines.join("\n") });
    }
    segments.push({ type: "conflict", block });
    cursor = block.endLine;
  }

  const trailing = lines.slice(cursor);
  if (trailing.length > 0) {
    segments.push({ type: "context", text: trailing.join("\n") });
  }

  return segments;
}

export type ConflictSide = "ours" | "theirs";

/**
 * Reconstructs the full single-version text for one side of a conflicted
 * file — i.e. what the file would look like if every conflict block were
 * resolved by taking only `ours` or only `theirs` — for use as a read-only
 * reference pane (IDE-style 3-way merge view). Also returns the 1-based,
 * inclusive line ranges within that reconstructed text that came from a
 * conflict block, so a caller can highlight them.
 */
export function buildSideText(
  text: string,
  side: ConflictSide,
): { text: string; ranges: { startLine: number; endLine: number }[] } {
  const segments = splitConflictSegments(text);
  const parts: string[] = [];
  const ranges: { startLine: number; endLine: number }[] = [];
  let line = 1;

  for (const seg of segments) {
    const content = seg.type === "context" ? seg.text : seg.block[side];
    const lineCount = content.split("\n").length;
    if (seg.type === "conflict") {
      ranges.push({ startLine: line, endLine: line + lineCount - 1 });
    }
    parts.push(content);
    line += lineCount;
  }

  return { text: parts.join("\n"), ranges };
}

/** Placeholder text substituted for each conflict block's raw marker lines
 * in the editable "result" pane of a 3-way merge view — a single plain
 * line, never raw `<<<<<<<` git syntax. */
export const CONFLICT_PLACEHOLDER_LINE = "⋯ конфликт не разрешён ⋯";

/**
 * Replaces every conflict block's raw marker lines with a single
 * {@link CONFLICT_PLACEHOLDER_LINE}, so an editable buffer never shows git's
 * `<<<<<<<`/`=======`/`>>>>>>>` syntax — the caller anchors an interactive
 * "pick a version" widget to each placeholder's line number instead.
 */
export function collapseConflictsToPlaceholders(
  text: string,
): { text: string; placeholders: { line: number; block: ConflictBlock }[] } {
  const segments = splitConflictSegments(text);
  const parts: string[] = [];
  const placeholders: { line: number; block: ConflictBlock }[] = [];
  let line = 1;

  for (const seg of segments) {
    if (seg.type === "context") {
      parts.push(seg.text);
      line += seg.text.split("\n").length;
    } else {
      placeholders.push({ line, block: seg.block });
      parts.push(CONFLICT_PLACEHOLDER_LINE);
      line += 1;
    }
  }

  return { text: parts.join("\n"), placeholders };
}

/** Mirrors the backend's marker check (`contains_conflict_markers` in
 * `git_repo.rs`) so the UI can catch a manually re-typed marker before
 * attempting to save. */
export function containsConflictMarkerLines(text: string): boolean {
  return text.split("\n").some(
    (line) =>
      line.startsWith("<<<<<<< ") ||
      line === "<<<<<<<" ||
      line.startsWith(">>>>>>> ") ||
      line === ">>>>>>>" ||
      line === "=======",
  );
}
