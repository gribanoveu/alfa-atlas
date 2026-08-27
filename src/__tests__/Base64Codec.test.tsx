import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";

afterEach(cleanup);

const copied: string[] = [];
mock.module("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: async (text: string) => {
    copied.push(text);
  },
}));

const { Base64Codec } = await import("../components/Utilities/Base64Codec");

describe("Base64Codec", () => {
  test("encode показывает Base64 для текста", () => {
    render(<Base64Codec />);
    fireEvent.change(screen.getByLabelText("Текст"), {
      target: { value: "docflow" },
    });

    expect(screen.getByLabelText("Результат").textContent).toBe("ZG9jZmxvdw==");
  });

  test("decode показывает текст для Base64", () => {
    render(<Base64Codec />);
    fireEvent.click(screen.getByRole("tab", { name: "Base64 → текст" }));
    fireEvent.change(screen.getByLabelText("Base64"), {
      target: { value: "ZG9jZmxvdw==" },
    });

    expect(screen.getByLabelText("Результат").textContent).toBe("docflow");
  });

  test("некорректный Base64 показывает ошибку", () => {
    render(<Base64Codec />);
    fireEvent.click(screen.getByRole("tab", { name: "Base64 → текст" }));
    fireEvent.change(screen.getByLabelText("Base64"), {
      target: { value: "***" },
    });

    expect(screen.getByRole("status").textContent).toBeTruthy();
  });

  test("копирование результата работает", async () => {
    copied.length = 0;
    render(<Base64Codec />);
    fireEvent.change(screen.getByLabelText("Текст"), {
      target: { value: "docflow" },
    });

    await act(async () => {
      fireEvent.click(screen.getByLabelText("Скопировать результат"));
    });

    expect(copied).toContain("ZG9jZmxvdw==");
  });
});
