import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";

afterEach(cleanup);

const copied: string[] = [];
mock.module("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: async (text: string) => {
    copied.push(text);
  },
}));

const { XmlFormatter } = await import("../components/Utilities/XmlFormatter");

describe("XmlFormatter", () => {
  test("prettify показывает отформатированный XML", () => {
    render(<XmlFormatter />);
    fireEvent.change(screen.getByLabelText("XML"), {
      target: { value: "<root><name>Alpha</name></root>" },
    });

    const output = screen.getByLabelText("Результат форматирования").textContent ?? "";
    expect(output).toContain("<root>");
    expect(output).toContain("  <name>Alpha</name>");
  });

  test("minify сжимает XML в одну строку", () => {
    render(<XmlFormatter />);
    fireEvent.click(screen.getByRole("tab", { name: "Minify" }));
    fireEvent.change(screen.getByLabelText("XML"), {
      target: { value: "<root>\n  <name>Alpha</name>\n</root>" },
    });

    const output = screen.getByLabelText("Результат форматирования").textContent ?? "";
    expect(output).not.toContain("\n");
    expect(output).toContain("<root><name>Alpha</name></root>");
  });

  test("некорректный XML показывает ошибку", () => {
    render(<XmlFormatter />);
    fireEvent.change(screen.getByLabelText("XML"), {
      target: { value: "<root><item>" },
    });

    expect(screen.getByRole("status").textContent).toBeTruthy();
  });

  test("копирование и замена входа работают", async () => {
    copied.length = 0;
    render(<XmlFormatter />);
    fireEvent.change(screen.getByLabelText("XML"), {
      target: { value: "<root><id>1</id></root>" },
    });

    await act(async () => {
      fireEvent.click(screen.getByLabelText("Скопировать результат"));
    });
    expect(copied.some((text) => text.includes("<id>1</id>"))).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Заменить вход" }));
    expect((screen.getByLabelText("XML") as HTMLTextAreaElement).value).toContain("<id>1</id>");
  });
});
