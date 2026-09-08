import { afterEach, describe, expect, mock, test } from "bun:test";
import { cleanup, render, waitFor } from "@testing-library/react";

mock.module("@tauri-apps/plugin-opener", () => ({ openUrl: async () => {} }));

// Тот же стаб и по той же причине, что в `AssistantVisualCard.test.tsx`:
// `mock.module` действует на весь процесс, поэтому подменяется `diagramRender`
// (его больше никто не импортирует), а не `mermaidRenderer`, который проверяет
// собственный тест со своим стабом mermaid.
mock.module("../lib/diagramRender", () => ({
  renderDiagram: async (_format: string, source: string) =>
    source.includes("INVALID")
      ? { kind: "error", message: "Syntax error in diagram" }
      : { kind: "ok", svg: '<svg xmlns="http://www.w3.org/2000/svg"></svg>' },
}));

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
  test("рисует ```mermaid как схему, а не как код", async () => {
    const { container } = render(
      <AssistantMarkdown content={"```mermaid\nflowchart LR\n  A --> B\n```"} streaming={false} />,
    );
    expect(container.querySelector(".markdown-code-block")).toBeNull();
    await waitFor(() => expect(container.querySelector(".markdown-mermaid-svg svg")).not.toBeNull());
  });

  for (const [name, content, expected] of cases) {
    test(`не теряет текст готового ответа: ${name}`, () => {
      const { container } = render(<AssistantMarkdown content={content} streaming={false} />);
      const text = container.textContent ?? "";
      for (const part of expected) expect(`${part} in «${text}»`).toContain(part);
      for (const part of expected) expect(text).toContain(part);
    });
  }
});
