import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";

afterEach(cleanup);

const copied: string[] = [];
mock.module("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: async (text: string) => {
    copied.push(text);
  },
}));

const { IdGenerator } = await import("../components/Utilities/IdGenerator");
const { isUuidV4 } = await import("../lib/uuid");
const { decodeUlidTimestamp, isUlid } = await import("../lib/ulid");

function rowValue(label: string): string {
  const labelNode = screen.getByText(label);
  const row = labelNode.closest(".idgen-row");
  if (!row) throw new Error(`Строка «${label}» не найдена`);
  return within(row as HTMLElement).getByText((_, node) =>
    node?.classList.contains("idgen-row-value") ?? false,
  ).textContent ?? "";
}

describe("IdGenerator", () => {
  test("по умолчанию показывает UUID v4", () => {
    render(<IdGenerator />);
    const value = rowValue("Значение");
    expect(isUuidV4(value)).toBe(true);
  });

  test("вкладка ULID показывает валидный ULID", () => {
    render(<IdGenerator />);
    fireEvent.click(screen.getByRole("tab", { name: "ULID" }));

    const value = rowValue("Значение");
    expect(isUlid(value)).toBe(true);
    expect(decodeUlidTimestamp(value)).not.toBeNull();
  });

  test("обновление генерирует новое значение", () => {
    render(<IdGenerator />);
    const before = rowValue("Значение");
    fireEvent.click(screen.getByRole("button", { name: "Обновить" }));
    const after = rowValue("Значение");
    expect(after).not.toBe(before);
  });

  test("копирование кладёт значение в буфер", async () => {
    copied.length = 0;
    render(<IdGenerator />);
    const value = rowValue("Значение");

    await act(async () => {
      fireEvent.click(screen.getByLabelText("Скопировать: Значение"));
    });

    expect(copied).toContain(value);
  });
});
