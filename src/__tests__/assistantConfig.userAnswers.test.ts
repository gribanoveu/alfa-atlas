import { describe, expect, test } from "bun:test";
import { buildUserAnswersContextBlock, USER_ANSWERS_CONTEXT_LIMIT } from "../lib/assistantConfig";
import type { ChatMessage, MessageBlock, ToolCallBlock } from "../lib/chatBlocks";

function askCall(
  questions: Array<{ id: string; prompt: string }>,
  answers: Array<{ questionId: string; selectedLabels?: string[]; customText?: string | null }>,
  status: ToolCallBlock["status"] = "done",
): MessageBlock {
  return {
    type: "toolCall",
    id: `ask_${questions.map((q) => q.id).join("_")}_${status}`,
    name: "askUser",
    argumentsJson: JSON.stringify({
      title: null,
      questions: questions.map((q) => ({ ...q, options: [], allowMultiple: false })),
    }),
    status,
    ...(status === "done"
      ? {
          result: {
            tool: "askUser" as const,
            result: {
              answers: answers.map((a) => ({
                questionId: a.questionId,
                selectedOptionIds: [],
                selectedLabels: a.selectedLabels ?? [],
                customText: a.customText ?? null,
              })),
            },
          },
        }
      : {}),
  } as MessageBlock;
}

function turn(...blocks: MessageBlock[]): ChatMessage {
  return { id: `m${Math.random()}`, role: "assistant", blocks, streaming: false };
}

describe("buildUserAnswersContextBlock", () => {
  test("says nothing when the user was never asked", () => {
    expect(buildUserAnswersContextBlock([])).toBeNull();
    expect(
      buildUserAnswersContextBlock([{ id: "u", role: "user", content: "привет" }]),
    ).toBeNull();
  });

  test("replays each answer next to the question it answered", () => {
    const block = buildUserAnswersContextBlock([
      turn(
        askCall(
          [
            { id: "q1", prompt: "Какой формат таблицы?" },
            { id: "q2", prompt: "Имя сервиса?" },
          ],
          [
            { questionId: "q1", selectedLabels: ["Расширенный"] },
            { questionId: "q2", customText: "ausn-transaction" },
          ],
        ),
      ),
    ])!;
    expect(block).toContain("[Answers]");
    expect(block).toContain("«Какой формат таблицы?» → «Расширенный»");
    expect(block).toContain("«Имя сервиса?» → «ausn-transaction»");
  });

  // A question the user skipped never settles into an answer, and a bare
  // "Расширенный" with no question in front of it is unusable a turn later.
  test("ignores calls that never settled and answers with no question", () => {
    expect(
      buildUserAnswersContextBlock([
        turn(askCall([{ id: "q1", prompt: "Формат?" }], [], "running")),
      ]),
    ).toBeNull();
    expect(
      buildUserAnswersContextBlock([
        turn(askCall([{ id: "q1", prompt: "Формат?" }], [{ questionId: "other", selectedLabels: ["X"] }])),
      ]),
    ).toBeNull();
    expect(
      buildUserAnswersContextBlock([
        turn(askCall([{ id: "q1", prompt: "Формат?" }], [{ questionId: "q1", selectedLabels: [] }])),
      ]),
    ).toBeNull();
  });

  // Re-asking means the user changed their mind; the later answer binds.
  test("keeps the latest answer to a re-asked question", () => {
    const block = buildUserAnswersContextBlock([
      turn(askCall([{ id: "q1", prompt: "Формат?" }], [{ questionId: "q1", selectedLabels: ["Краткий"] }])),
      turn(askCall([{ id: "q1", prompt: "Формат?" }], [{ questionId: "q1", selectedLabels: ["Расширенный"] }])),
    ])!;
    expect(block).toContain("«Расширенный»");
    expect(block).not.toContain("«Краткий»");
  });

  test("keeps the most recent answers when a chat accumulates too many", () => {
    const turns = Array.from({ length: USER_ANSWERS_CONTEXT_LIMIT + 5 }, (_, i) =>
      turn(
        askCall([{ id: `q${i}`, prompt: `Вопрос ${i}` }], [
          { questionId: `q${i}`, selectedLabels: [`Ответ ${i}`] },
        ]),
      ),
    );
    const block = buildUserAnswersContextBlock(turns)!;
    expect(block.split("\n- ").length - 1).toBe(USER_ANSWERS_CONTEXT_LIMIT);
    expect(block).toContain(`Ответ ${USER_ANSWERS_CONTEXT_LIMIT + 4}`);
    expect(block).not.toContain("Ответ 0");
  });
});
