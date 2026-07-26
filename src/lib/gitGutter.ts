import { diffChars, diffLines } from "diff";

export type GitGutterHunkKind = "added" | "modified" | "deleted";

export type GitGutterHunk = {
  id: string;
  kind: GitGutterHunkKind;
  /** Range in the current document; for deleted — insertion anchor line. */
  startLine: number;
  endLine: number;
  baselineText: string;
  currentText: string;
};

function countLines(text: string): number {
  if (text.length === 0) return 0;
  const parts = text.split("\n");
  if (parts.length > 1 && parts[parts.length - 1] === "") {
    return parts.length - 1;
  }
  return parts.length;
}

function splitLines(text: string): string[] {
  if (text.length === 0) return [];
  const parts = text.split("\n");
  if (parts.length > 1 && parts[parts.length - 1] === "") {
    parts.pop();
  }
  return parts;
}

function joinLines(lines: string[]): string {
  if (lines.length === 0) return "";
  return `${lines.join("\n")}\n`;
}

export function computeGitGutterHunks(
  baseline: string,
  current: string,
): GitGutterHunk[] {
  const changes = diffLines(baseline, current);
  const hunks: GitGutterHunk[] = [];
  let currentLine = 1;
  let hunkIndex = 0;
  let i = 0;

  while (i < changes.length) {
    const change = changes[i];

    if (!change.added && !change.removed) {
      currentLine += countLines(change.value);
      i += 1;
      continue;
    }

    const next = changes[i + 1];
    if (change.removed && next?.added) {
      const currentLineCount = countLines(next.value);
      const startLine = currentLine;
      const endLine =
        currentLineCount > 0 ? currentLine + currentLineCount - 1 : currentLine;
      hunks.push({
        id: `h${hunkIndex++}`,
        kind: "modified",
        startLine,
        endLine,
        baselineText: change.value,
        currentText: next.value,
      });
      currentLine += currentLineCount;
      i += 2;
      continue;
    }

    if (change.removed) {
      hunks.push({
        id: `h${hunkIndex++}`,
        kind: "deleted",
        startLine: currentLine,
        endLine: currentLine,
        baselineText: change.value,
        currentText: "",
      });
      i += 1;
      continue;
    }

    if (change.added) {
      const currentLineCount = countLines(change.value);
      hunks.push({
        id: `h${hunkIndex++}`,
        kind: "added",
        startLine: currentLine,
        endLine: currentLine + Math.max(0, currentLineCount - 1),
        baselineText: "",
        currentText: change.value,
      });
      currentLine += currentLineCount;
      i += 1;
    }
  }

  return hunks;
}

export function applyHunkRevert(current: string, hunk: GitGutterHunk): string {
  const lines = splitLines(current);

  switch (hunk.kind) {
    case "added": {
      const before = lines.slice(0, hunk.startLine - 1);
      const after = lines.slice(hunk.endLine);
      return joinLines([...before, ...after]);
    }
    case "modified": {
      const baselineLines = splitLines(hunk.baselineText);
      const before = lines.slice(0, hunk.startLine - 1);
      const after = lines.slice(hunk.endLine);
      return joinLines([...before, ...baselineLines, ...after]);
    }
    case "deleted": {
      const baselineLines = splitLines(hunk.baselineText);
      const before = lines.slice(0, hunk.startLine - 1);
      const after = lines.slice(hunk.startLine - 1);
      return joinLines([...before, ...baselineLines, ...after]);
    }
  }
}

export function hunkDecorationClass(kind: GitGutterHunkKind): string {
  switch (kind) {
    case "added":
      return "git-gutter-added";
    case "modified":
      return "git-gutter-modified";
    case "deleted":
      return "git-gutter-deleted";
  }
}

export function findHunkAtLine(
  hunks: GitGutterHunk[],
  line: number,
): GitGutterHunk | null {
  for (const hunk of hunks) {
    if (hunk.kind === "deleted") {
      if (line === hunk.startLine || line === hunk.startLine - 1) return hunk;
      continue;
    }
    if (line >= hunk.startLine && line <= hunk.endLine) return hunk;
  }
  return null;
}

export type HunkDiffPart = {
  kind: "same" | "removed" | "added";
  text: string;
};

export type HunkDiffLine = {
  kind: "removed" | "added";
  parts: HunkDiffPart[];
};

function charDiffParts(
  removed: string,
  added: string,
): { removedParts: HunkDiffPart[]; addedParts: HunkDiffPart[] } {
  const removedParts: HunkDiffPart[] = [];
  const addedParts: HunkDiffPart[] = [];

  for (const change of diffChars(removed, added)) {
    if (change.removed) {
      removedParts.push({ kind: "removed", text: change.value });
    } else if (change.added) {
      addedParts.push({ kind: "added", text: change.value });
    } else {
      removedParts.push({ kind: "same", text: change.value });
      addedParts.push({ kind: "same", text: change.value });
    }
  }

  return { removedParts, addedParts };
}

function wholeLineParts(
  kind: "removed" | "added",
  text: string,
): HunkDiffPart[] {
  return [{ kind, text }];
}

export function buildHunkDiffLines(
  baseline: string,
  current: string,
): HunkDiffLine[] {
  const lines: HunkDiffLine[] = [];
  const changes = diffLines(baseline, current);
  let i = 0;

  while (i < changes.length) {
    const change = changes[i];
    if (!change.added && !change.removed) {
      i += 1;
      continue;
    }

    const next = changes[i + 1];
    if (change.removed && next?.added) {
      const removedLines = splitLines(change.value);
      const addedLines = splitLines(next.value);
      const lineCount = Math.max(removedLines.length, addedLines.length);

      for (let lineIndex = 0; lineIndex < lineCount; lineIndex += 1) {
        const removedText = removedLines[lineIndex] ?? "";
        const addedText = addedLines[lineIndex] ?? "";

        if (removedText && addedText) {
          const { removedParts, addedParts } = charDiffParts(
            removedText,
            addedText,
          );
          lines.push({ kind: "removed", parts: removedParts });
          lines.push({ kind: "added", parts: addedParts });
        } else if (removedText) {
          lines.push({
            kind: "removed",
            parts: wholeLineParts("removed", removedText),
          });
        } else if (addedText) {
          lines.push({
            kind: "added",
            parts: wholeLineParts("added", addedText),
          });
        }
      }

      i += 2;
      continue;
    }

    const kind = change.removed ? "removed" : "added";
    for (const text of splitLines(change.value)) {
      lines.push({ kind, parts: wholeLineParts(kind, text) });
    }
    i += 1;
  }

  return lines;
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function renderDiffParts(parts: HunkDiffPart[]): string {
  return parts
    .map((part) => {
      const className =
        part.kind === "same"
          ? "git-gutter-diff-same"
          : part.kind === "removed"
            ? "git-gutter-diff-char-removed"
            : "git-gutter-diff-char-added";
      return `<span class="${className}">${escapeHtml(part.text)}</span>`;
    })
    .join("");
}

export function renderHunkDiffHtml(lines: HunkDiffLine[]): string {
  return lines
    .map(
      (line) =>
        `<div class="git-gutter-diff-line git-gutter-diff-${line.kind}">${renderDiffParts(line.parts)}</div>`,
    )
    .join("");
}
