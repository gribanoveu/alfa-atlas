import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "bun:test";
import {
  AssistantActivityGroup,
  formatStepCount,
} from "../components/RightDock/AssistantActivityGroup";
import type { MessageBlock, ToolCallBlock } from "../lib/chatBlocks";

afterEach(cleanup);

const NO_LIVE_BLOCKS: ReadonlySet<string> = new Set<string>();

function call(
  id: string,
  name: string,
  status: ToolCallBlock["status"],
  argumentsJson = "{}",
): ToolCallBlock {
  return { type: "toolCall", id, name, argumentsJson, status };
}

function think(id: string): MessageBlock {
  return { type: "reasoning", id, content: "внутренние размышления" };
}

function renderGroup(blocks: MessageBlock[], active: boolean) {
  return render(
    <AssistantActivityGroup
      blocks={blocks}
      active={active}
      liveBlockIds={NO_LIVE_BLOCKS}
      liveKind={undefined}
    />,
  );
}

describe("formatStepCount", () => {
  test("declines the noun instead of always saying «шагов»", () => {
    expect(formatStepCount(1)).toBe("1 шаг");
    expect(formatStepCount(3)).toBe("3 шага");
    expect(formatStepCount(5)).toBe("5 шагов");
    expect(formatStepCount(11)).toBe("11 шагов");
    expect(formatStepCount(21)).toBe("21 шаг");
    expect(formatStepCount(114)).toBe("114 шагов");
  });
});

describe("AssistantActivityGroup", () => {
  test("a live run shows the step in flight, not the whole list", () => {
    renderGroup(
      [
        think("r1"),
        call("t1", "semanticSearch", "done", '{"query":"оплата"}'),
        call("t2", "readFile", "running", '{"path":"docs/api/pay.adoc"}'),
      ],
      true,
    );

    expect(screen.getByText("Читает файл: pay.adoc…")).toBeTruthy();
    // The run's own cards must not be in the document while collapsed —
    // that is the wall of machinery this component exists to replace.
    expect(screen.queryByText("внутренние размышления")).toBeNull();
  });

  test("between two calls it says the model is thinking", () => {
    renderGroup([think("r1"), call("t1", "readFile", "done")], true);
    expect(screen.getByText(/…$/)).toBeTruthy();
    expect(screen.queryByText("Ход работы")).toBeNull();
  });

  test("a settled run collapses to a step count", () => {
    renderGroup(
      [think("r1"), call("t1", "readFile", "done"), call("t2", "grep", "done")],
      false,
    );
    expect(screen.getByText("Ход работы")).toBeTruthy();
    // Reasoning is not a step: the count is about what the assistant did.
    expect(screen.getByText("2 шага")).toBeTruthy();
  });

  test("opening it renders every block that was folded away", () => {
    renderGroup(
      [think("r1"), call("t1", "readFile", "done"), call("t2", "grep", "done")],
      false,
    );

    fireEvent.click(screen.getByRole("button", { expanded: false }));

    // Exactly the cards that were there before the run was folded — each
    // still its own collapsed disclosure, down to the reasoning text.
    expect(screen.getByText("Ход рассуждений")).toBeTruthy();
    expect(screen.getByText("Читает файл…")).toBeTruthy();
    expect(screen.getByText("Ищет по regex…")).toBeTruthy();

    fireEvent.click(screen.getByText("Ход рассуждений"));
    expect(screen.getByText("внутренние размышления")).toBeTruthy();
  });

  test("a failed call is announced on the closed row", () => {
    // A tool error must never be something the user has to go hunting for.
    renderGroup([call("t1", "readFile", "done"), call("t2", "grep", "error")], false);
    expect(screen.getByTitle(/ошибкой/)).toBeTruthy();
  });
});
