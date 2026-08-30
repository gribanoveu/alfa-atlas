import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";

afterEach(cleanup);

const copied: string[] = [];
mock.module("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: async (text: string) => {
    copied.push(text);
  },
}));

const { AssistantUserMessage } = await import("../components/RightDock/AssistantUserMessage");

describe("AssistantUserMessage", () => {
  test("копирует текст отправленного сообщения и подтверждает копирование", async () => {
    copied.length = 0;
    render(<AssistantUserMessage content="Опиши модуль импорта" />);

    expect(screen.getByText("Опиши модуль импорта")).toBeDefined();

    await act(async () => {
      fireEvent.click(screen.getByLabelText("Копировать сообщение"));
    });

    expect(copied).toEqual(["Опиши модуль импорта"]);
    expect(screen.getByLabelText("Скопировано")).toBeDefined();
  });
});
