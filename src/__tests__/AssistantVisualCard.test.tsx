import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, mock, test } from "bun:test";
import type { ToolCallBlock } from "../lib/chatBlocks";

// Stubbed at `lib/diagramRender`, the dispatch module nothing else imports.
// `mock.module` is process-wide, and `mermaidRenderer.test.ts` exercises the
// real `renderMermaid` against its own `mermaid` stub — mocking that here
// would make the two files fight over load order. It also keeps mermaid
// (~600 kB) and the TeaVM PlantUML engine (~6 MB) out of a test about what
// the card does with a render result.
mock.module("../lib/diagramRender", () => ({
  renderDiagram: async (_format: string, source: string) =>
    source.includes("INVALID")
      ? { kind: "error", message: "Syntax error in diagram" }
      : { kind: "ok", svg: '<svg xmlns="http://www.w3.org/2000/svg"></svg>' },
}));

const { AssistantVisualCard, renderFailureNote } = await import(
  "../components/RightDock/AssistantVisualCard"
);

afterEach(cleanup);

function block(overrides: Partial<ToolCallBlock> = {}): ToolCallBlock {
  return {
    type: "toolCall",
    id: "call-1",
    name: "visualize",
    argumentsJson: "",
    status: "running",
    ...overrides,
  };
}

/** A settled call, the shape `visualFromBlock` reads. */
function drawn(source: string, title = "Оплата"): ToolCallBlock {
  return block({
    status: "done",
    argumentsJson: JSON.stringify({ kind: "diagram", title, format: "mermaid", source }),
    result: {
      tool: "visualShown",
      result: { visualId: "v1", kind: "diagram", title, summary: "mermaid diagram" },
    },
  } as Partial<ToolCallBlock>);
}

function noop() {}

describe("AssistantVisualCard — while the call is in flight", () => {
  test("shows a loader card and no diagram source", () => {
    render(
      <AssistantVisualCard
        block={block({
          argumentsJson: '{"kind":"diagram","title":"Оплата","source":"flowchart TD\\n  A-->B',
        })}
        turnActive
        onOpenVisual={noop}
        onRenderError={noop}
        onRedraw={noop}
      />,
    );
    expect(screen.getByText("Рисую схему…")).toBeTruthy();
    expect(screen.queryByLabelText("Исходник схемы")).toBeNull();
    expect(screen.queryByText(/flowchart/)).toBeNull();
  });
});

describe("AssistantVisualCard — a settled call", () => {
  test("draws the diagram in the card rather than only naming it", async () => {
    render(
      <AssistantVisualCard
        block={drawn("flowchart TD\n  A-->B")}
        turnActive={false}
        onOpenVisual={noop}
        onRenderError={noop}
        onRedraw={noop}
      />,
    );
    await waitFor(() => expect(screen.getByLabelText("Оплата")).toBeTruthy());
    expect(screen.getByText("Схема готова")).toBeTruthy();
    expect(screen.getByText("Просмотр")).toBeTruthy();
  });

  test("a source that does not parse reports the failure to the model mid-turn", async () => {
    const notes: string[] = [];
    render(
      <AssistantVisualCard
        block={drawn("sequenceDiagram\n  INVALID")}
        turnActive
        onOpenVisual={noop}
        onRenderError={(note) => notes.push(note)}
        onRedraw={noop}
      />,
    );
    await waitFor(() => expect(screen.getByText("Syntax error in diagram")).toBeTruthy());
    // The card stops claiming success, and offers the way back.
    expect(screen.queryByText("Схема готова")).toBeNull();
    expect(screen.getByText("Перерисовать")).toBeTruthy();
    expect(notes).toEqual([renderFailureNote("Оплата", "Syntax error in diagram")]);
  });

  test("does not report a failure once the turn is over — the note would be dropped", async () => {
    const notes: string[] = [];
    render(
      <AssistantVisualCard
        block={drawn("sequenceDiagram\n  INVALID")}
        turnActive={false}
        onOpenVisual={noop}
        onRenderError={(note) => notes.push(note)}
        onRedraw={noop}
      />,
    );
    await waitFor(() => expect(screen.getByText("Перерисовать")).toBeTruthy());
    expect(notes).toEqual([]);
  });
});
