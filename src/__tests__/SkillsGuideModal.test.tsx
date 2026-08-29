import { afterEach, describe, expect, mock, test } from "bun:test";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

afterEach(cleanup);

// `mock.module` is process-wide: keep the whole plugin surface other test
// files rely on, not just the one function this component imports.
mock.module("@tauri-apps/plugin-opener", () => ({
  openUrl: async () => {},
  openPath: async () => {},
}));

const { SkillsGuideModal } = await import("../components/Settings/SkillsGuideModal");

describe("SkillsGuideModal", () => {
  test("рендерит руководство разметкой, а не сырым текстом", () => {
    render(<SkillsGuideModal onClose={() => {}} />);

    expect(screen.getByText("Формат SKILL.md").tagName).toBe("H2");
    // Пример SKILL.md показан кодом, таблица полей — таблицей.
    expect(document.querySelector(".markdown-code-block")).not.toBeNull();
    expect(document.querySelector(".markdown-table")).not.toBeNull();
    expect(document.body.textContent).not.toContain("---\nname: method-spec");
  });

  test("Escape закрывает окно", () => {
    let closed = false;
    render(<SkillsGuideModal onClose={() => (closed = true)} />);

    fireEvent.keyDown(document, { key: "Escape" });

    expect(closed).toBe(true);
  });
});
