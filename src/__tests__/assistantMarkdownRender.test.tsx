import { afterEach, describe, expect, mock, test } from "bun:test";
import { cleanup, render } from "@testing-library/react";

mock.module("@tauri-apps/plugin-opener", () => ({ openUrl: async () => {} }));

const { AssistantMarkdown } = await import("../components/RightDock/AssistantMarkdown");

afterEach(cleanup);

/** Streamdown's `parseIncompleteMarkdown` hides half-written markup while a
 * message streams. That is right mid-stream and wrong once the answer is
 * final: text the model actually wrote must survive to the transcript. */
const cases: Array<[name: string, content: string, expected: string[]]> = [
  ["квадратная скобка", "Возьми arr[0 и посмотри результат", ["arr[0", "результат"]],
  ["незакрытая ссылка", "Смотри [документацию по импорту", ["документацию по импорту"]],
  ["незакрытый жирный", "Итог: **важно", ["важно"]],
  ["незакрытый фенс", "Код:\n```ts\nconst a = 1;", ["const a = 1;"]],
  ["подчёркивания в именах", "файл chat_store.rs и llm_chat.rs", ["chat_store", "llm_chat"]],
  ["таблица и хвост", "| a | b |\n|---|---|\n| 1 | 2 |\n\nхвост", ["хвост"]],
];

describe("AssistantMarkdown", () => {
  for (const [name, content, expected] of cases) {
    test(`не теряет текст готового ответа: ${name}`, () => {
      const { container } = render(<AssistantMarkdown content={content} streaming={false} />);
      const text = container.textContent ?? "";
      for (const part of expected) expect(`${part} in «${text}»`).toContain(part);
      for (const part of expected) expect(text).toContain(part);
    });
  }
});
