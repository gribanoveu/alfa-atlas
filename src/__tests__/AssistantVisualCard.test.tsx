import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "bun:test";
import { AssistantVisualCard } from "../components/RightDock/AssistantVisualCard";
import type { ToolCallBlock } from "../lib/chatBlocks";

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

describe("AssistantVisualCard — while the call is in flight", () => {
  test("shows a loader card and no diagram source", () => {
    render(
      <AssistantVisualCard
        block={block({
          argumentsJson: '{"kind":"diagram","title":"Оплата","source":"flowchart TD\\n  A-->B',
        })}
        onOpenVisual={() => {}}
      />,
    );
    expect(screen.getByText("Рисую схему…")).toBeTruthy();
    expect(screen.queryByLabelText("Исходник схемы")).toBeNull();
    expect(screen.queryByText(/flowchart/)).toBeNull();
  });
});
