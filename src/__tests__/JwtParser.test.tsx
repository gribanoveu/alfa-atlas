import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";

afterEach(cleanup);

const copied: string[] = [];
mock.module("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: async (text: string) => {
    copied.push(text);
  },
}));

const SAMPLE =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2HT4EpwuHnKz-zZX0";

const { JwtParser } = await import("../components/Utilities/JwtParser");

describe("JwtParser", () => {
  test("показывает header и payload после вставки токена", () => {
    render(<JwtParser />);
    fireEvent.change(screen.getByLabelText("JWT"), { target: { value: SAMPLE } });

    expect(screen.getByText("Header")).toBeDefined();
    expect(screen.getByText("Payload")).toBeDefined();
    expect(screen.getByText("Claims")).toBeDefined();
    expect(screen.getByText("HS256")).toBeDefined();
    expect(screen.getByText("sub")).toBeDefined();
  });

  test("мусорный ввод показывает ошибку", () => {
    render(<JwtParser />);
    fireEvent.change(screen.getByLabelText("JWT"), { target: { value: "not-a-jwt" } });

    expect(screen.getByRole("status").textContent).toContain("трёх частей");
  });

  test("копирование payload кладёт JSON в буфер", async () => {
    copied.length = 0;
    render(<JwtParser />);
    fireEvent.change(screen.getByLabelText("JWT"), { target: { value: SAMPLE } });

    await act(async () => {
      fireEvent.click(screen.getByLabelText("Скопировать: Payload"));
    });

    expect(copied.some((text) => text.includes('"name": "John Doe"'))).toBe(true);
  });
});
