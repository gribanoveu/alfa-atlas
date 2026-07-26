import { describe, expect, mock, test } from "bun:test";

if (typeof globalThis.window === "undefined") {
  (globalThis as typeof globalThis & { window: typeof globalThis }).window =
    globalThis;
}

mock.module("mermaid", () => ({
  default: {
    initialize: () => {},
    render: async (_id: string, text: string) => {
      if (text.includes("INVALID")) {
        throw new Error("Syntax error in diagram");
      }
      return { svg: '<svg xmlns="http://www.w3.org/2000/svg"></svg>' };
    },
  },
}));

const { normalizeMermaidSource, renderMermaid } = await import(
  "../components/AsciiDocPreview/mermaidRenderer"
);

describe("normalizeMermaidSource", () => {
  test("trims leading and trailing blank lines", () => {
    expect(normalizeMermaidSource("\n\nflowchart TD\n  A --> B\n\n")).toBe(
      "flowchart TD\n  A --> B",
    );
  });

  test("preserves internal blank lines", () => {
    expect(normalizeMermaidSource("flowchart TD\n\n  A --> B")).toBe(
      "flowchart TD\n\n  A --> B",
    );
  });
});

describe("renderMermaid", () => {
  test("returns error for empty source", async () => {
    const result = await renderMermaid("   \n  ");
    expect(result).toEqual({ kind: "error", message: "Mermaid diagram is empty" });
  });

  test("returns svg on success", async () => {
    const result = await renderMermaid("flowchart TD\n  A --> B");
    expect(result.kind).toBe("ok");
    if (result.kind === "ok") {
      expect(result.svg).toContain("<svg");
    }
  });

  test("returns error message when mermaid throws", async () => {
    const result = await renderMermaid("INVALID diagram");
    expect(result).toEqual({
      kind: "error",
      message: "Syntax error in diagram",
    });
  });
});
