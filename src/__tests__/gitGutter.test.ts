import { describe, expect, test } from "bun:test";
import {
  applyHunkRevert,
  buildHunkDiffLines,
  computeGitGutterHunks,
  findHunkAtLine,
  renderHunkDiffHtml,
} from "../lib/gitGutter";

describe("computeGitGutterHunks", () => {
  test("added lines", () => {
    const hunks = computeGitGutterHunks("one\n", "one\ntwo\nthree\n");
    expect(hunks).toHaveLength(1);
    expect(hunks[0].kind).toBe("added");
    expect(hunks[0].startLine).toBe(2);
    expect(hunks[0].endLine).toBe(3);
    expect(hunks[0].currentText).toBe("two\nthree\n");
  });

  test("deleted lines", () => {
    const hunks = computeGitGutterHunks("one\ntwo\nthree\n", "one\nthree\n");
    expect(hunks).toHaveLength(1);
    expect(hunks[0].kind).toBe("deleted");
    expect(hunks[0].startLine).toBe(2);
    expect(hunks[0].baselineText).toBe("two\n");
  });

  test("modified lines", () => {
    const hunks = computeGitGutterHunks("alpha\nbeta\n", "alpha\nBETA\n");
    expect(hunks).toHaveLength(1);
    expect(hunks[0].kind).toBe("modified");
    expect(hunks[0].startLine).toBe(2);
    expect(hunks[0].endLine).toBe(2);
    expect(hunks[0].baselineText).toBe("beta\n");
    expect(hunks[0].currentText).toBe("BETA\n");
  });

  test("mixed hunks", () => {
    const baseline = "a\nb\nc\nd\n";
    const current = "a\nB\ne\nd\n";
    const hunks = computeGitGutterHunks(baseline, current);
    expect(hunks.length).toBeGreaterThanOrEqual(1);
    expect(hunks.some((h) => h.kind === "modified" || h.kind === "added")).toBe(
      true,
    );
  });

  test("untracked file — all added", () => {
    const hunks = computeGitGutterHunks("", "hello\nworld\n");
    expect(hunks).toHaveLength(1);
    expect(hunks[0].kind).toBe("added");
    expect(hunks[0].startLine).toBe(1);
    expect(hunks[0].endLine).toBe(2);
  });
});

describe("applyHunkRevert", () => {
  test("reverts added hunk", () => {
    const current = "one\ntwo\nthree\n";
    const hunks = computeGitGutterHunks("one\n", current);
    const added = hunks.find((h) => h.kind === "added");
    expect(added).toBeDefined();
    expect(applyHunkRevert(current, added!)).toBe("one\n");
  });

  test("reverts modified hunk", () => {
    const baseline = "alpha\nbeta\n";
    const current = "alpha\nBETA\n";
    const hunks = computeGitGutterHunks(baseline, current);
    expect(applyHunkRevert(current, hunks[0])).toBe(baseline);
  });

  test("reverts deleted hunk", () => {
    const baseline = "one\ntwo\nthree\n";
    const current = "one\nthree\n";
    const hunks = computeGitGutterHunks(baseline, current);
    expect(applyHunkRevert(current, hunks[0])).toBe(baseline);
  });

  test("partial revert leaves other hunks", () => {
    const baseline = "a\nb\nc\nd\n";
    const current = "a\nB\nc\nD\n";
    const hunks = computeGitGutterHunks(baseline, current);
    expect(hunks.length).toBe(2);
    const afterFirst = applyHunkRevert(current, hunks[0]);
    expect(afterFirst).not.toBe(baseline);
    expect(afterFirst).not.toBe(current);
  });
});

describe("findHunkAtLine", () => {
  test("finds hunk on changed line", () => {
    const hunks = computeGitGutterHunks("one\n", "one\ntwo\n");
    expect(findHunkAtLine(hunks, 2)?.kind).toBe("added");
    expect(findHunkAtLine(hunks, 1)).toBeNull();
  });
});

describe("buildHunkDiffLines", () => {
  test("modified hunk shows removed and added lines with char parts", () => {
    const lines = buildHunkDiffLines(
      "=== Входные параметры\n",
      "=== Входные параметрыа\n",
    );
    expect(lines).toHaveLength(2);
    expect(lines[0]).toEqual({
      kind: "removed",
      parts: [{ kind: "same", text: "=== Входные параметры" }],
    });
    expect(lines[1]).toEqual({
      kind: "added",
      parts: [
        { kind: "same", text: "=== Входные параметры" },
        { kind: "added", text: "а" },
      ],
    });
  });

  test("modified line highlights replaced characters", () => {
    const lines = buildHunkDiffLines("alpha\n", "BETA\n");
    expect(lines[0].parts).toEqual([{ kind: "removed", text: "alpha" }]);
    expect(lines[1].parts).toEqual([{ kind: "added", text: "BETA" }]);
  });

  test("added-only hunk shows only added lines", () => {
    const lines = buildHunkDiffLines("", "hello\nworld\n");
    expect(lines).toEqual([
      { kind: "added", parts: [{ kind: "added", text: "hello" }] },
      { kind: "added", parts: [{ kind: "added", text: "world" }] },
    ]);
  });

  test("deleted-only hunk shows only removed lines", () => {
    const lines = buildHunkDiffLines("one\ntwo\n", "one\n");
    expect(lines).toEqual([
      { kind: "removed", parts: [{ kind: "removed", text: "two" }] },
    ]);
  });
});

describe("renderHunkDiffHtml", () => {
  test("escapes HTML in diff text", () => {
    const html = renderHunkDiffHtml([
      { kind: "removed", parts: [{ kind: "removed", text: "<tag>" }] },
      { kind: "added", parts: [{ kind: "added", text: "a & b" }] },
    ]);
    expect(html).toContain("&lt;tag&gt;");
    expect(html).toContain("a &amp; b");
    expect(html).toContain("git-gutter-diff-char-removed");
    expect(html).toContain("git-gutter-diff-char-added");
  });

  test("highlights char-level additions", () => {
    const html = renderHunkDiffHtml([
      {
        kind: "added",
        parts: [
          { kind: "same", text: "foo" },
          { kind: "added", text: "bar" },
        ],
      },
    ]);
    expect(html).toContain('class="git-gutter-diff-same"');
    expect(html).toContain('class="git-gutter-diff-char-added"');
    expect(html).toContain("bar");
  });
});
