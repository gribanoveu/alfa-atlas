import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";

afterEach(cleanup);

const copied: string[] = [];
mock.module("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: async (text: string) => {
    copied.push(text);
  },
}));

const { JsonDiff } = await import("../components/Utilities/JsonDiff");

describe("JsonDiff", () => {
  test("показывает изменения по путям и построчный diff", () => {
    render(<JsonDiff />);

    fireEvent.change(screen.getByLabelText("Исходный JSON"), {
      target: { value: '{"name":"Alpha","id":1}' },
    });
    fireEvent.change(screen.getByLabelText("Новый JSON"), {
      target: { value: '{"name":"Beta","id":1,"active":true}' },
    });

    expect(screen.getByText("Изменения по путям")).toBeDefined();
    expect(screen.getByText("$.name")).toBeDefined();
    expect(screen.getByText("$.active")).toBeDefined();
    expect(screen.getByLabelText("Построчный diff").textContent).toContain('"name": "Beta"');
  });

  test("одинаковый JSON показывает сообщение о совпадении", () => {
    render(<JsonDiff />);

    fireEvent.change(screen.getByLabelText("Исходный JSON"), {
      target: { value: '{"same":true}' },
    });
    fireEvent.change(screen.getByLabelText("Новый JSON"), {
      target: { value: '{"same":true}' },
    });

    expect(screen.getByRole("status").textContent).toContain("совпадает");
  });

  test("некорректный JSON показывает ошибку", () => {
    render(<JsonDiff />);

    fireEvent.change(screen.getByLabelText("Исходный JSON"), {
      target: { value: "{bad" },
    });

    expect(screen.getByRole("status").textContent).toContain("Исходный JSON");
  });

  test("копирование diff кладёт unified diff в буфер", async () => {
    copied.length = 0;
    render(<JsonDiff />);

    fireEvent.change(screen.getByLabelText("Исходный JSON"), {
      target: { value: '{"a":1}' },
    });
    fireEvent.change(screen.getByLabelText("Новый JSON"), {
      target: { value: '{"a":2}' },
    });

    await act(async () => {
      fireEvent.click(screen.getByLabelText("Скопировать diff"));
    });

    expect(copied.some((text) => text.includes('"a": 2'))).toBe(true);
  });
});
