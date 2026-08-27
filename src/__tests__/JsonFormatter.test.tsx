import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";

afterEach(cleanup);

const copied: string[] = [];
mock.module("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: async (text: string) => {
    copied.push(text);
  },
}));

const { JsonFormatter } = await import("../components/Utilities/JsonFormatter");

describe("JsonFormatter", () => {
  test("prettify показывает отформатированный JSON", () => {
    render(<JsonFormatter />);
    fireEvent.change(screen.getByLabelText("JSON"), {
      target: { value: '{"b":2,"a":1}' },
    });

    expect(screen.getByLabelText("Результат форматирования").textContent).toContain('"a": 1');
    expect(screen.getByLabelText("Результат форматирования").textContent).toContain('\n');
  });

  test("minify сжимает JSON в одну строку", () => {
    render(<JsonFormatter />);
    fireEvent.click(screen.getByRole("tab", { name: "Minify" }));
    fireEvent.change(screen.getByLabelText("JSON"), {
      target: { value: '{\n  "a": 1\n}' },
    });

    expect(screen.getByLabelText("Результат форматирования").textContent).toBe('{"a":1}');
  });

  test("сортировка ключей меняет порядок", () => {
    render(<JsonFormatter />);
    fireEvent.click(screen.getByRole("tab", { name: "Да" }));
    fireEvent.change(screen.getByLabelText("JSON"), {
      target: { value: '{"z":1,"a":2}' },
    });

    const output = screen.getByLabelText("Результат форматирования").textContent ?? "";
    expect(output.indexOf('"a"')).toBeLessThan(output.indexOf('"z"'));
  });

  test("некорректный JSON показывает ошибку", () => {
    render(<JsonFormatter />);
    fireEvent.change(screen.getByLabelText("JSON"), {
      target: { value: "{bad" },
    });

    expect(screen.getByRole("status").textContent).toBeTruthy();
  });

  test("копирование и замена входа работают", async () => {
    copied.length = 0;
    render(<JsonFormatter />);
    fireEvent.change(screen.getByLabelText("JSON"), {
      target: { value: '{"a":1}' },
    });

    await act(async () => {
      fireEvent.click(screen.getByLabelText("Скопировать результат"));
    });
    expect(copied.some((text) => text.includes('"a": 1'))).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Заменить вход" }));
    expect((screen.getByLabelText("JSON") as HTMLTextAreaElement).value).toContain('"a": 1');
  });
});
