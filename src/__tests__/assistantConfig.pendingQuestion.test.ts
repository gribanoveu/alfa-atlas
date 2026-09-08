import { describe, expect, test } from "bun:test";
import { buildPendingQuestionContextBlock } from "../lib/assistantConfig";
import type { ChatMessage } from "../lib/chatBlocks";

function assistant(...paragraphs: string[]): ChatMessage {
  return {
    id: `a${Math.random()}`,
    role: "assistant",
    streaming: false,
    blocks: paragraphs.map((content, i) => ({ type: "text", id: `t${i}`, content })),
  };
}

const USER: ChatMessage = { id: "u1", role: "user", content: "приведи в порядок" };

const OFFER = "Хочешь — проверю остальные методы репозитория на те же проблемы?";

describe("buildPendingQuestionContextBlock", () => {
  test("names the question the acknowledgement answers", () => {
    const block = buildPendingQuestionContextBlock([USER, assistant("Готово.", OFFER)], "да")!;
    expect(block).toContain("[Reply]");
    expect(block).toContain(OFFER);
    expect(block).toContain("«да»");
  });

  test.each(["да", "Да!", "ок", "давай", "да давай", "ОК, го"])(
    "fires on the acknowledgement %p",
    (reply) => {
      expect(buildPendingQuestionContextBlock([assistant(OFFER)], reply)).not.toBeNull();
    },
  );

  // A reply with substance of its own already names its subject; a reminder
  // there would only compete with it.
  test("says nothing when the reply carries its own content", () => {
    expect(
      buildPendingQuestionContextBlock([assistant(OFFER)], "да, но сначала посмотри addEmployee"),
    ).toBeNull();
    expect(buildPendingQuestionContextBlock([assistant(OFFER)], "не надо")).toBeNull();
    expect(buildPendingQuestionContextBlock([assistant(OFFER)], "")).toBeNull();
  });

  test("says nothing when the previous turn asked nothing", () => {
    expect(
      buildPendingQuestionContextBlock([assistant("Готово. Все 8 фолдеров прошли проверку.")], "да"),
    ).toBeNull();
    expect(buildPendingQuestionContextBlock([], "да")).toBeNull();
    expect(buildPendingQuestionContextBlock([USER], "да")).toBeNull();
  });

  // Pointing at the wrong question is the failure this exists to prevent,
  // so it anchors on the last line rather than the first `?` it finds.
  test("takes the closing question, not an earlier one in the body", () => {
    const block = buildPendingQuestionContextBlock(
      [assistant("| Метод | Исправить? |\n| addEmployee | да |", "Что дальше — правим?")],
      "да",
    )!;
    expect(block).toContain("Что дальше — правим?");
    expect(block).not.toContain("Исправить?");
  });

  test("trims a question too long to quote whole", () => {
    const long = `${"о".repeat(600)}?`;
    const block = buildPendingQuestionContextBlock([assistant(long)], "да")!;
    expect(block).toContain("…");
    expect(block.length).toBeLessThan(900);
  });
});
