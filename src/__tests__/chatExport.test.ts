import { describe, expect, test } from "bun:test";
import { chatMessagesToMarkdown } from "../lib/chatExport";
import type { ChatMessage, ToolCallBlock } from "../lib/chatBlocks";

function assistant(blocks: ToolCallBlock[]): ChatMessage {
  return { id: "m1", role: "assistant", blocks };
}

function call(overrides: Partial<ToolCallBlock>): ToolCallBlock {
  return {
    type: "toolCall",
    id: "c1",
    name: "readFile",
    argumentsJson: "{}",
    status: "done",
    ...overrides,
  } as ToolCallBlock;
}

describe("chatMessagesToMarkdown — tool results", () => {
  test("an askUser call carries the answer, not just the question", () => {
    const md = chatMessagesToMarkdown(
      "Разбор",
      [
        assistant([
          call({
            name: "askUser",
            argumentsJson: JSON.stringify({ title: "Выбор диаграммы" }),
            result: {
              tool: "askUser",
              result: {
                answers: [
                  {
                    questionId: "diagram_choice",
                    selectedOptionIds: ["statuses"],
                    selectedLabels: ["Жизненный цикл статусов"],
                    customText: null,
                  },
                ],
              },
            },
          }),
        ]),
      ],
      [],
    );
    // Without the result line this reads as a question the assistant asked
    // and then ignored — which is exactly how it looked in the transcript
    // that prompted this.
    expect(md).toContain("Ответ: Жизненный цикл статусов");
  });

  test("a diagram that was drawn is distinguishable from a turn that drew nothing", () => {
    const md = chatMessagesToMarkdown(
      "Разбор",
      [
        assistant([
          call({
            name: "visualize",
            result: {
              tool: "visualShown",
              result: {
                visualId: "v1",
                kind: "diagram",
                title: "Поток подписи",
                summary: "mermaid diagram, 12 lines, rendered in a tab",
              },
            },
          }),
        ]),
      ],
      [],
    );
    expect(md).toContain("Схема: Поток подписи");
  });

  test("an unsettled call gets no result line — the status already says why", () => {
    const md = chatMessagesToMarkdown(
      "Разбор",
      [assistant([call({ status: "error", errorMessage: "no task with id: t1" })])],
      [],
    );
    expect(md).toContain("no task with id: t1");
    expect(md).not.toContain("_Результат:");
  });
});
