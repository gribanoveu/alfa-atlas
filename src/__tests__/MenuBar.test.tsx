import { afterEach, describe, expect, test } from "bun:test";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MenuBar } from "../components/TopBar/MenuBar";
import type { EditAvailability } from "../lib/editClipboard";
import type { MenuActionId } from "../lib/menuActions";

afterEach(cleanup);

function openEditMenu(availability?: EditAvailability) {
  const actions: MenuActionId[] = [];
  render(
    <MenuBar
      onAction={(action) => actions.push(action)}
      hasActiveTab
      getEditAvailability={availability ? () => availability : undefined}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Правка" }));
  return actions;
}

const item = (label: string) => screen.getByRole("menuitem", { name: label }) as HTMLButtonElement;

describe("меню «Правка»", () => {
  test("пункты включены по тому, что доступно при открытии меню", () => {
    openEditMenu({ cut: false, copy: true, paste: true });

    expect(item("Вырезать").disabled).toBe(true);
    expect(item("Копировать").disabled).toBe(false);
    expect(item("Вставить").disabled).toBe(false);
  });

  test("клик отправляет действие", () => {
    const actions = openEditMenu({ cut: true, copy: true, paste: true });

    fireEvent.click(item("Вырезать"));
    expect(actions).toEqual(["edit.cut"]);
  });

  test("выключенный пункт ничего не отправляет", () => {
    const actions = openEditMenu({ cut: false, copy: false, paste: false });

    fireEvent.click(item("Копировать"));
    expect(actions).toEqual([]);
  });

  test("без источника доступности буфер недоступен", () => {
    openEditMenu();

    expect(item("Вырезать").disabled).toBe(true);
    expect(item("Копировать").disabled).toBe(true);
    expect(item("Вставить").disabled).toBe(true);
    // Undo/Redo живут по своему правилу и открытым документом включены.
    expect(item("Отменить").disabled).toBe(false);
  });
});
