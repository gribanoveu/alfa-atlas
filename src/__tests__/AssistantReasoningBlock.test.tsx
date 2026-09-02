import { afterEach, describe, expect, test } from "bun:test";
import { cleanup, render, screen } from "@testing-library/react";

import {
  AssistantReasoningBlock,
  formatReasoningSize,
} from "../components/RightDock/AssistantReasoningBlock";
import type { ReasoningBlock } from "../lib/chatBlocks";

afterEach(cleanup);

function block(content: string): ReasoningBlock {
  return { type: "reasoning", id: "r1", content };
}

describe("formatReasoningSize", () => {
  test("an empty trace shows nothing rather than a zero", () => {
    expect(formatReasoningSize(0)).toBeNull();
  });

  test("a short trace is exact", () => {
    expect(formatReasoningSize(842)).toBe("842 зн.");
  });

  test("past a thousand it keeps one decimal", () => {
    expect(formatReasoningSize(4137)).toBe("4,1к зн.");
  });

  test("past ten thousand it drops the decimal — the slot is narrow", () => {
    expect(formatReasoningSize(23480)).toBe("23к зн.");
  });
});

describe("AssistantReasoningBlock", () => {
  test("a collapsed card reports how much has been written into it", () => {
    render(<AssistantReasoningBlock block={block("x".repeat(1200))} thinking />);
    // The whole point: readable without opening the card.
    expect(screen.getByText("1,2к зн.")).toBeDefined();
    expect(screen.queryByText("x".repeat(1200))).toBeNull();
  });

  test("the count grows as the trace does", () => {
    const { rerender } = render(<AssistantReasoningBlock block={block("x".repeat(120))} thinking />);
    expect(screen.getByText("120 зн.")).toBeDefined();
    rerender(<AssistantReasoningBlock block={block("x".repeat(340))} thinking />);
    expect(screen.getByText("340 зн.")).toBeDefined();
  });

  test("a trace restored from history still reports its size", () => {
    // Unlike the elapsed timer, which must stay hidden there.
    render(<AssistantReasoningBlock block={block("x".repeat(500))} thinking={false} />);
    expect(screen.getByText("500 зн.")).toBeDefined();
  });
});
