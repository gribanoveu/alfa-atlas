import { describe, expect, test } from "bun:test";
import {
  buildSideText,
  CONFLICT_PLACEHOLDER_LINE,
  collapseConflictsToPlaceholders,
  containsConflictMarkerLines,
  countConflictBlocks,
  parseConflictBlocks,
  splitConflictSegments,
} from "../lib/gitConflict";

describe("parseConflictBlocks", () => {
  test("no markers returns empty", () => {
    expect(parseConflictBlocks("plain text\nno conflicts\n")).toEqual([]);
  });

  test("single conflict block", () => {
    const text = [
      "before",
      "<<<<<<< HEAD",
      "our line",
      "=======",
      "their line",
      ">>>>>>> origin/main",
      "after",
    ].join("\n");

    const blocks = parseConflictBlocks(text);
    expect(blocks).toHaveLength(1);
    expect(blocks[0]).toEqual({
      startLine: 2,
      midLine: 4,
      endLine: 6,
      ours: "our line",
      theirs: "their line",
    });
  });

  test("multiple conflict blocks", () => {
    const text = [
      "<<<<<<< HEAD",
      "a1",
      "=======",
      "b1",
      ">>>>>>> theirs",
      "middle",
      "<<<<<<< HEAD",
      "a2",
      "=======",
      "b2",
      ">>>>>>> theirs",
    ].join("\n");

    const blocks = parseConflictBlocks(text);
    expect(blocks).toHaveLength(2);
    expect(blocks[0].ours).toBe("a1");
    expect(blocks[1].theirs).toBe("b2");
  });

  test("multi-line sides", () => {
    const text = [
      "<<<<<<< HEAD",
      "a1",
      "a2",
      "=======",
      "b1",
      ">>>>>>> theirs",
    ].join("\n");

    const blocks = parseConflictBlocks(text);
    expect(blocks[0].ours).toBe("a1\na2");
  });

  test("unterminated marker is ignored", () => {
    const text = ["<<<<<<< HEAD", "a1", "======="].join("\n");
    expect(parseConflictBlocks(text)).toEqual([]);
  });

  test("countConflictBlocks matches parsed count", () => {
    const text = [
      "<<<<<<< HEAD",
      "a",
      "=======",
      "b",
      ">>>>>>> theirs",
    ].join("\n");
    expect(countConflictBlocks(text)).toBe(1);
    expect(countConflictBlocks("no conflicts here")).toBe(0);
  });
});

describe("splitConflictSegments", () => {
  test("no markers returns a single context segment", () => {
    const text = "plain text\nno conflicts\n";
    expect(splitConflictSegments(text)).toEqual([{ type: "context", text }]);
  });

  test("splits leading/trailing context around a single block", () => {
    const text = [
      "before",
      "<<<<<<< HEAD",
      "our line",
      "=======",
      "their line",
      ">>>>>>> origin/main",
      "after",
    ].join("\n");

    const segments = splitConflictSegments(text);
    expect(segments).toHaveLength(3);
    expect(segments[0]).toEqual({ type: "context", text: "before" });
    expect(segments[1].type).toBe("conflict");
    if (segments[1].type === "conflict") {
      expect(segments[1].block.ours).toBe("our line");
      expect(segments[1].block.theirs).toBe("their line");
    }
    expect(segments[2]).toEqual({ type: "context", text: "after" });
  });

  test("no leading/trailing context when the block spans the whole file", () => {
    const text = ["<<<<<<< HEAD", "a", "=======", "b", ">>>>>>> theirs"].join("\n");
    const segments = splitConflictSegments(text);
    expect(segments).toHaveLength(1);
    expect(segments[0].type).toBe("conflict");
  });

  test("middle context between two blocks is preserved", () => {
    const text = [
      "<<<<<<< HEAD",
      "a1",
      "=======",
      "b1",
      ">>>>>>> theirs",
      "middle",
      "<<<<<<< HEAD",
      "a2",
      "=======",
      "b2",
      ">>>>>>> theirs",
    ].join("\n");

    const segments = splitConflictSegments(text);
    expect(segments.map((s) => s.type)).toEqual(["conflict", "context", "conflict"]);
    expect(segments[1]).toEqual({ type: "context", text: "middle" });
  });

  test("segments round-trip to the original text when joined with \\n", () => {
    const text = [
      "before",
      "<<<<<<< HEAD",
      "our line",
      "=======",
      "their line",
      ">>>>>>> origin/main",
      "middle",
      "<<<<<<< HEAD",
      "a2",
      "=======",
      "b2",
      ">>>>>>> theirs",
      "after",
    ].join("\n");

    const segments = splitConflictSegments(text);
    const rebuilt = segments
      .map((seg) =>
        seg.type === "context"
          ? seg.text
          : [
              "<<<<<<< HEAD",
              seg.block.ours,
              "=======",
              seg.block.theirs,
              ">>>>>>> theirs",
            ].join("\n"),
      )
      .join("\n");
    expect(rebuilt).toBe(text.replace(">>>>>>> origin/main", ">>>>>>> theirs"));
  });
});

describe("containsConflictMarkerLines", () => {
  test("plain text has no markers", () => {
    expect(containsConflictMarkerLines("plain text\nno conflicts\n")).toBe(false);
  });

  test("detects a leftover start marker", () => {
    expect(containsConflictMarkerLines("<<<<<<< HEAD\nsome text")).toBe(true);
  });

  test("detects a leftover separator", () => {
    expect(containsConflictMarkerLines("resolved text\n=======\nmore text")).toBe(true);
  });

  test("detects a leftover end marker", () => {
    expect(containsConflictMarkerLines("resolved text\n>>>>>>> origin/main")).toBe(true);
  });

  test("bare markers without trailing label are also detected", () => {
    expect(containsConflictMarkerLines("<<<<<<<\ntext\n>>>>>>>")).toBe(true);
  });
});

describe("buildSideText", () => {
  test("no conflicts returns the text unchanged with no ranges", () => {
    const text = "plain text\nno conflicts\n";
    expect(buildSideText(text, "ours")).toEqual({ text, ranges: [] });
  });

  test("picks ours/theirs content and reports its line range", () => {
    const text = [
      "before",
      "<<<<<<< HEAD",
      "our line",
      "=======",
      "their line",
      ">>>>>>> origin/main",
      "after",
    ].join("\n");

    const ours = buildSideText(text, "ours");
    expect(ours.text).toBe(["before", "our line", "after"].join("\n"));
    expect(ours.ranges).toEqual([{ startLine: 2, endLine: 2 }]);

    const theirs = buildSideText(text, "theirs");
    expect(theirs.text).toBe(["before", "their line", "after"].join("\n"));
    expect(theirs.ranges).toEqual([{ startLine: 2, endLine: 2 }]);
  });

  test("multi-line side content produces a multi-line range", () => {
    const text = [
      "before",
      "<<<<<<< HEAD",
      "a1",
      "a2",
      "a3",
      "=======",
      "b1",
      ">>>>>>> theirs",
      "after",
    ].join("\n");

    const ours = buildSideText(text, "ours");
    expect(ours.text).toBe(["before", "a1", "a2", "a3", "after"].join("\n"));
    expect(ours.ranges).toEqual([{ startLine: 2, endLine: 4 }]);
  });

  test("multiple blocks each get their own range at the right offset", () => {
    const text = [
      "<<<<<<< HEAD",
      "a1",
      "=======",
      "b1",
      ">>>>>>> theirs",
      "middle",
      "<<<<<<< HEAD",
      "a2",
      "a2b",
      "=======",
      "b2",
      ">>>>>>> theirs",
    ].join("\n");

    const ours = buildSideText(text, "ours");
    expect(ours.text).toBe(["a1", "middle", "a2", "a2b"].join("\n"));
    expect(ours.ranges).toEqual([
      { startLine: 1, endLine: 1 },
      { startLine: 3, endLine: 4 },
    ]);
  });

  test("empty side content still reports a valid single-line range", () => {
    const text = ["<<<<<<< HEAD", "=======", "incoming", ">>>>>>> theirs"].join("\n");
    const ours = buildSideText(text, "ours");
    expect(ours.text).toBe("");
    expect(ours.ranges).toEqual([{ startLine: 1, endLine: 1 }]);
  });
});

describe("collapseConflictsToPlaceholders", () => {
  test("no conflicts returns the text unchanged with no placeholders", () => {
    const text = "plain text\nno conflicts\n";
    expect(collapseConflictsToPlaceholders(text)).toEqual({ text, placeholders: [] });
  });

  test("never leaves raw marker syntax in the collapsed text", () => {
    const text = [
      "before",
      "<<<<<<< HEAD",
      "our line",
      "=======",
      "their line",
      ">>>>>>> origin/main",
      "after",
    ].join("\n");

    const { text: collapsed } = collapseConflictsToPlaceholders(text);
    expect(collapsed).not.toContain("<<<<<<<");
    expect(collapsed).not.toContain("=======");
    expect(collapsed).not.toContain(">>>>>>>");
    expect(collapsed).toBe(["before", CONFLICT_PLACEHOLDER_LINE, "after"].join("\n"));
  });

  test("reports the placeholder's line number and original block data", () => {
    const text = [
      "before",
      "<<<<<<< HEAD",
      "our line",
      "=======",
      "their line",
      ">>>>>>> origin/main",
      "after",
    ].join("\n");

    const { placeholders } = collapseConflictsToPlaceholders(text);
    expect(placeholders).toHaveLength(1);
    expect(placeholders[0].line).toBe(2);
    expect(placeholders[0].block.ours).toBe("our line");
    expect(placeholders[0].block.theirs).toBe("their line");
  });

  test("multiple blocks each get their own placeholder at the right line", () => {
    const text = [
      "<<<<<<< HEAD",
      "a1",
      "=======",
      "b1",
      ">>>>>>> theirs",
      "middle",
      "<<<<<<< HEAD",
      "a2",
      "a2b",
      "=======",
      "b2",
      ">>>>>>> theirs",
    ].join("\n");

    const { text: collapsed, placeholders } = collapseConflictsToPlaceholders(text);
    expect(collapsed).toBe(
      [CONFLICT_PLACEHOLDER_LINE, "middle", CONFLICT_PLACEHOLDER_LINE].join("\n"),
    );
    expect(placeholders.map((p) => p.line)).toEqual([1, 3]);
  });
});
