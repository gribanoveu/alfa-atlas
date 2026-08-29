import { describe, expect, test } from "bun:test";
import type { ToolCallBlock } from "../lib/chatBlocks";
import { visualFromBlock, visualIdFromTabId, visualTabId } from "../lib/visuals";

const ARGS = JSON.stringify({
  kind: "diagram",
  title: "Поток данных",
  caption: "Слева направо",
  format: "mermaid",
  source: "flowchart TD\n  a-->b",
});

function block(overrides: Partial<ToolCallBlock> = {}): ToolCallBlock {
  return {
    type: "toolCall",
    id: "call-1",
    name: "visualize",
    argumentsJson: ARGS,
    status: "done",
    result: {
      tool: "visualShown",
      result: {
        visualId: "v1",
        kind: "diagram",
        title: "Поток данных",
        summary: "mermaid diagram, 2 lines, rendered in a tab",
      },
    },
    ...overrides,
  };
}

describe("visualTabId / visualIdFromTabId", () => {
  test("they round-trip an id", () => {
    expect(visualIdFromTabId(visualTabId("v1"))).toBe("v1");
  });

  test("other tab-id families are not claimed", () => {
    // The strip routes by prefix, so a false positive here would hijack
    // another pane's tab.
    expect(visualIdFromTabId("a.adoc")).toBeNull();
    expect(visualIdFromTabId("artifact:abc")).toBeNull();
    expect(visualIdFromTabId("plan:abc")).toBeNull();
    expect(visualIdFromTabId("visual:")).toBeNull();
  });
});

describe("visualFromBlock", () => {
  test("it joins the result's id with the call's own source", () => {
    // The whole storage story: the id comes from the backend result, the
    // content comes back out of the arguments the chat already persists.
    expect(visualFromBlock(block())).toEqual({
      id: "v1",
      title: "Поток данных",
      caption: "Слева направо",
      content: { kind: "diagram", format: "mermaid", source: "flowchart TD\n  a-->b" },
    });
  });

  test("a missing caption is left off rather than empty", () => {
    const args = JSON.stringify({
      kind: "diagram",
      title: "x",
      format: "plantuml",
      source: "@startuml\n@enduml",
    });
    const visual = visualFromBlock(block({ argumentsJson: args }));
    expect(visual?.caption).toBeUndefined();
    expect(visual?.content.format).toBe("plantuml");
  });

  test("a blank caption is treated as no caption", () => {
    const args = JSON.stringify({ kind: "diagram", title: "x", caption: "   ", format: "mermaid", source: "flowchart TD" });
    expect(visualFromBlock(block({ argumentsJson: args }))?.caption).toBeUndefined();
  });

  test("an unsettled or failed call has nothing to open", () => {
    expect(visualFromBlock(block({ status: "running", result: undefined }))).toBeNull();
    expect(visualFromBlock(block({ status: "error", result: undefined, errorMessage: "boom" }))).toBeNull();
    expect(visualFromBlock(block({ status: "pendingApproval", result: undefined }))).toBeNull();
  });

  test("a result from some other tool is not mistaken for a visualization", () => {
    const other = block({
      result: { tool: "planRead", result: { planId: "p", name: "n", overview: "o", plan: "", todos: [] } },
    });
    expect(visualFromBlock(other)).toBeNull();
  });

  test("unreadable arguments make the block unrenderable", () => {
    // The result deliberately carries no source, so there is no fallback:
    // broken arguments mean the diagram is genuinely gone.
    expect(visualFromBlock(block({ argumentsJson: "{not json" }))).toBeNull();
    expect(visualFromBlock(block({ argumentsJson: "{}" }))).toBeNull();
  });

  test("a kind or format the app cannot render is rejected", () => {
    const badKind = JSON.stringify({ kind: "spreadsheet", title: "x", format: "mermaid", source: "a" });
    expect(visualFromBlock(block({ argumentsJson: badKind }))).toBeNull();

    const badFormat = JSON.stringify({ kind: "diagram", title: "x", format: "graphviz", source: "a" });
    expect(visualFromBlock(block({ argumentsJson: badFormat }))).toBeNull();
  });

  test("an empty source is rejected rather than opening a blank tab", () => {
    const args = JSON.stringify({ kind: "diagram", title: "x", format: "mermaid", source: "   \n " });
    expect(visualFromBlock(block({ argumentsJson: args }))).toBeNull();
  });

  test("it falls back to the arguments' title when the result carries none", () => {
    const stripped = block({
      result: { tool: "visualShown", result: { visualId: "v1", kind: "diagram", title: "", summary: "" } },
    });
    expect(visualFromBlock(stripped)?.title).toBe("Поток данных");
  });
});
