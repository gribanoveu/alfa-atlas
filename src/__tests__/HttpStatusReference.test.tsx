import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";

afterEach(cleanup);

const copied: string[] = [];
mock.module("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: async (text: string) => {
    copied.push(text);
  },
}));

const { HttpStatusReference } = await import("../components/Utilities/HttpStatusReference");

describe("HttpStatusReference", () => {
  test("показывает коды, сгруппированные по классам", () => {
    render(<HttpStatusReference />);

    expect(screen.getByText("2xx — успех")).toBeDefined();
    expect(screen.getByText("Not Found")).toBeDefined();
    expect(screen.getByText("502")).toBeDefined();
  });

  test("поиск скрывает нерелевантные коды", () => {
    render(<HttpStatusReference />);
    fireEvent.change(screen.getByLabelText("Поиск HTTP-кода"), {
      target: { value: "404" },
    });

    expect(screen.getByText("Not Found")).toBeDefined();
    expect(screen.queryByText("Bad Gateway")).toBeNull();
  });

  test("копирование кладёт код в буфер", async () => {
    copied.length = 0;
    render(<HttpStatusReference />);
    fireEvent.change(screen.getByLabelText("Поиск HTTP-кода"), {
      target: { value: "404" },
    });

    await act(async () => {
      fireEvent.click(screen.getByLabelText("Скопировать код 404"));
    });

    expect(copied).toContain("404");
  });
});
