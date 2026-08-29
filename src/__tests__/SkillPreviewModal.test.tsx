import { afterEach, describe, expect, mock, test } from "bun:test";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { SkillListItem } from "../lib/skills";
import * as actualSkills from "../lib/skills";

afterEach(cleanup);

const SKILL_MD = "---\nname: method-spec\ndescription: Fills REST docs.\n---\n# Заголовок\n\nТекст.\n";

mock.module("../lib/skills", () => ({
  ...actualSkills,
  skillsFiles: async () => ["SKILL.md", "assets/template.adoc"],
  skillsReadFile: async (_source: string, _name: string, path: string) =>
    path === "SKILL.md" ? SKILL_MD : "= AsciiDoc\n",
}));
// `mock.module` is process-wide: keep the whole plugin surface other test
// files rely on, not just the one function this component imports.
mock.module("@tauri-apps/plugin-opener", () => ({
  openUrl: async () => {},
  openPath: async () => {},
}));

const { SkillPreviewModal } = await import("../components/Settings/SkillPreviewModal");

const skill: SkillListItem = {
  name: "method-spec",
  description: "Fills REST docs.",
  source: "bundled",
  enabled: true,
  error: null,
};

describe("SkillPreviewModal", () => {
  test("рендерит SKILL.md разметкой, вынося frontmatter из тела", async () => {
    render(<SkillPreviewModal skill={skill} onClose={() => {}} />);

    await waitFor(() => expect(screen.getByText("Заголовок")).toBeDefined());
    // Frontmatter shows as metadata, not as a heading of the document.
    expect(screen.getByText(/description: Fills REST docs\./)).toBeDefined();
    expect(screen.getByText("Заголовок").tagName).toBe("H1");
    expect(screen.getByText("Текст.")).toBeDefined();
  });

  test("переключение на исходник показывает файл целиком", async () => {
    render(<SkillPreviewModal skill={skill} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText("Заголовок")).toBeDefined());

    fireEvent.click(screen.getByText("Исходник"));

    await waitFor(() => expect(screen.queryByText("Заголовок")).toBeNull());
    expect(document.body.textContent).toContain("name: method-spec");
  });

  test("выбор другого файла загружает его", async () => {
    render(<SkillPreviewModal skill={skill} onClose={() => {}} />);
    await waitFor(() => expect(screen.getByText("Заголовок")).toBeDefined());

    fireEvent.click(screen.getByText("assets/template.adoc"));

    await waitFor(() => expect(document.body.textContent).toContain("= AsciiDoc"));
    // A non-Markdown file has nothing to toggle between.
    expect(screen.queryByText("Разметка")).toBeNull();
  });

  test("Escape закрывает окно", async () => {
    let closed = false;
    render(<SkillPreviewModal skill={skill} onClose={() => (closed = true)} />);

    fireEvent.keyDown(document, { key: "Escape" });

    expect(closed).toBe(true);
  });
});
