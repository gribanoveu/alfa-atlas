import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";

// Bun's runner does not register testing-library's auto-cleanup, so without
// this every render stacks in `document.body` and `screen` starts matching
// several copies at once.
afterEach(cleanup);

const copied: string[] = [];
mock.module("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: async (text: string) => {
    copied.push(text);
  },
}));

const { UnixtimeConverter } = await import("../components/Utilities/UnixtimeConverter");

/** Значение строки результата по её подписи (подписи уникальны в секции). */
function rowValue(label: string): string {
  const labelNode = screen.getAllByText(label)[0];
  const row = labelNode.closest(".unix-row");
  if (!row) throw new Error(`Строка «${label}» не найдена`);
  return within(row as HTMLElement).getByText((_, node) =>
    node?.classList.contains("unix-row-value") ?? false,
  ).textContent ?? "";
}

function typeUnix(value: string) {
  fireEvent.change(screen.getByLabelText("Unix-время"), { target: { value } });
}

function setField(label: string, value: string) {
  fireEvent.change(screen.getByLabelText(label), { target: { value } });
}

function openEncodeTab() {
  fireEvent.click(screen.getByRole("tab", { name: "Дата → Unixtime" }));
}

describe("UnixtimeConverter", () => {
  test("секунды разворачиваются в ISO 8601 и обе нормализованные метки", () => {
    render(<UnixtimeConverter />);
    typeUnix("1700000000");

    expect(rowValue("ISO 8601 (UTC)")).toBe("2023-11-14T22:13:20.000Z");
    expect(rowValue("Unix (сек)")).toBe("1700000000");
    expect(rowValue("Timestamp (мс)")).toBe("1700000000000");
  });

  test("миллисекундная метка распознаётся без переключения режима", () => {
    render(<UnixtimeConverter />);
    typeUnix("1700000000000");

    expect(screen.getByText("Timestamp (миллисекунды)")).toBeDefined();
    expect(rowValue("ISO 8601 (UTC)")).toBe("2023-11-14T22:13:20.000Z");
  });

  test("явный выбор единицы перебивает автоопределение", () => {
    render(<UnixtimeConverter />);
    typeUnix("1700000000");
    fireEvent.click(screen.getByRole("button", { name: "Миллисекунды" }));

    expect(rowValue("ISO 8601 (UTC)")).toBe("1970-01-20T16:13:20.000Z");
  });

  test("мусорный ввод показывает ошибку вместо строк результата", () => {
    const { container } = render(<UnixtimeConverter />);
    typeUnix("вчера");

    const decodePanel = container.querySelectorAll(".unix-panel")[0];
    expect(decodePanel.querySelector(".unix-error")?.textContent).toBe(
      "Ожидается число: только цифры, знак и точка",
    );
    expect(decodePanel.querySelectorAll(".unix-row")).toHaveLength(0);
  });

  test("собранная руками дата в UTC переводится в unix и timestamp", () => {
    render(<UnixtimeConverter />);
    openEncodeTab();
    fireEvent.click(screen.getByRole("button", { name: "UTC" }));
    setField("Год", "2023");
    setField("Месяц", "11");
    setField("День", "14");
    setField("Часы", "22");
    setField("Мин", "13");
    setField("Сек", "20");
    setField("Мс", "0");

    const panel = document.querySelector(".unix-panel");
    const encode = within(panel as HTMLElement);
    const value = (label: string) =>
      encode
        .getByText(label)
        .closest(".unix-row")
        ?.querySelector(".unix-row-value")?.textContent;

    expect(value("Unix (сек)")).toBe("1700000000");
    expect(value("Timestamp (мс)")).toBe("1700000000000");
    expect(value("ISO 8601 (UTC)")).toBe("2023-11-14T22:13:20.000Z");
  });

  test("несуществующая дата объясняется, а не переносится на следующий месяц", () => {
    render(<UnixtimeConverter />);
    openEncodeTab();
    fireEvent.click(screen.getByRole("button", { name: "UTC" }));
    setField("Месяц", "2");
    setField("День", "31");

    expect(screen.getByText("Такой даты не существует")).toBeDefined();
  });

  test("пустое поле не считается нулём", () => {
    render(<UnixtimeConverter />);
    openEncodeTab();
    setField("Год", "");

    expect(screen.getByText("Заполните все поля целыми числами")).toBeDefined();
  });

  test("копирование кладёт в буфер именно значение строки", async () => {
    copied.length = 0;
    render(<UnixtimeConverter />);
    typeUnix("1700000000");

    // Запись в буфер асинхронна, а «Скопировано» ставится уже после await —
    // без act этот setState прилетит за пределами рендера.
    await act(async () => {
      fireEvent.click(screen.getAllByLabelText("Скопировать: ISO 8601 (UTC)")[0]);
    });

    expect(copied).toContain("2023-11-14T22:13:20.000Z");
  });
});
