import { describe, expect, mock, test } from "bun:test";

if (typeof globalThis.window === "undefined") {
  (globalThis as typeof globalThis & { window: typeof globalThis }).window =
    globalThis;
}

const initCalls: { theme: string }[] = [];

mock.module("mermaid", () => ({
  default: {
    initialize: (config: { theme: string }) => {
      initCalls.push(config);
    },
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

describe("renderMermaid theming", () => {
  test("maps the app's palette onto mermaid's built-in themes", async () => {
    initCalls.length = 0;
    await renderMermaid("flowchart TD\n  A --> B", "dark");
    await renderMermaid("flowchart TD\n  A --> B", "light");
    expect(initCalls.map((c) => c.theme)).toEqual(["dark", "default"]);
  });

  test("defaults to the dark palette", async () => {
    initCalls.length = 0;
    await renderMermaid("flowchart TD\n  A --> B");
    expect(initCalls.at(-1)?.theme).toBe("dark");
  });

  test("re-initializes per render so a theme change takes effect", async () => {
    // `initialize` is the only way to change mermaid's theme, and the
    // module is shared — this is why it runs per render rather than once
    // at load, and why the render queue has to serialize it.
    initCalls.length = 0;
    await renderMermaid("flowchart TD\n  A --> B", "light");
    await renderMermaid("flowchart TD\n  A --> B", "light");
    expect(initCalls.length).toBe(2);
  });

  test("an empty source never reaches the engine", async () => {
    initCalls.length = 0;
    await renderMermaid("  \n ", "dark");
    expect(initCalls).toEqual([]);
  });
});
