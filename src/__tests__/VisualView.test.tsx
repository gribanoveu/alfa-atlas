import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { AbstractBlock } from "../components/AsciiDocPreview/types";
import type { Visual } from "../lib/visuals";
import * as actualFileSave from "../lib/fileSave";

afterEach(cleanup);

// Stubbed at the *viewer* layer, not the renderer layer: `mock.module` is
// process-wide, and `mermaidRenderer.test.ts` exercises the real
// `renderMermaid` against its own `mermaid` stub — mocking either of those
// here would make the two files fight over load order. Nothing else under
// test imports these two components, so this is the collision-free seam.
// It also keeps mermaid (~600 kB) and the TeaVM PlantUML engine (~6 MB) out
// of a test that is about the tab's shell — header, toggle, save, and which
// engine gets asked to draw.
mock.module("../components/AsciiDocPreview/AscMermaid", () => ({
  AscMermaid: ({ block }: { block: AbstractBlock }) => (
    <div data-engine="mermaid">{block.getSource()}</div>
  ),
}));
mock.module("../components/AsciiDocPreview/AscPlantuml", () => ({
  AscPlantuml: ({ block }: { block: AbstractBlock }) => (
    <div data-engine="plantuml">{block.getSource()}</div>
  ),
}));

const saveCalls: { defaultPath?: string; bytes: Uint8Array }[] = [];
let saveResult = true;
// Re-registered per test rather than once at import: `mock.module` is
// process-wide and last-registration-wins, so another file mocking
// `fileSave` would otherwise silently decide which version is live here
// based on test-file ordering. Spread of the real module for the same
// process-wide reason in reverse — whatever this leaves out, it takes away
// from every other file too.
beforeEach(() => {
  mock.module("../lib/fileSave", () => ({
    ...actualFileSave,
    saveBytesViaDialog: async (bytes: Uint8Array, options: { defaultPath?: string }) => {
      saveCalls.push({ bytes, defaultPath: options.defaultPath });
      return saveResult;
    },
  }));
});

const { VisualView } = await import("../components/Visuals/VisualView");

const SOURCE = "flowchart TD\n  a-->b";

function visual(overrides: Partial<Visual> = {}): Visual {
  return {
    id: "v1",
    title: "Поток данных",
    caption: "Слева направо",
    content: { kind: "diagram", format: "mermaid", source: SOURCE },
    ...overrides,
  };
}

describe("VisualView", () => {
  test("показывает заголовок, подпись и формат", () => {
    render(<VisualView visual={visual()} />);
    expect(screen.getByText("Поток данных")).toBeDefined();
    expect(screen.getByText("Слева направо")).toBeDefined();
    expect(screen.getByText(/Визуализация · Схема · Mermaid/)).toBeDefined();
  });

  test("по умолчанию рисует схему mermaid-движком", () => {
    const { container } = render(<VisualView visual={visual()} />);
    const drawn = container.querySelector("[data-engine]");
    expect(drawn?.getAttribute("data-engine")).toBe("mermaid");
    expect(drawn?.textContent).toBe(SOURCE);
  });

  test("формат plantuml уходит в plantuml-движок", () => {
    const puml = "@startuml\nAlice -> Bob\n@enduml";
    const { container } = render(
      <VisualView visual={visual({ content: { kind: "diagram", format: "plantuml", source: puml } })} />,
    );
    const drawn = container.querySelector("[data-engine]");
    expect(drawn?.getAttribute("data-engine")).toBe("plantuml");
    expect(drawn?.textContent).toBe(puml);
  });

  test("переключатель показывает исходник и возвращает обратно к схеме", () => {
    render(<VisualView visual={visual()} />);
    fireEvent.click(screen.getByText("Исходник"));
    expect(screen.getByText("flowchart TD")).toBeDefined();
    expect(screen.getByText("Исходник").getAttribute("aria-pressed")).toBe("true");

    fireEvent.click(screen.getByText("Схема"));
    expect(screen.getByText("Схема").getAttribute("aria-pressed")).toBe("true");
  });

  test("сохранение предлагает имя из заголовка и расширение формата", async () => {
    saveCalls.length = 0;
    saveResult = true;
    render(<VisualView visual={visual()} />);
    fireEvent.click(screen.getByText("Сохранить в файл"));

    await waitFor(() => expect(saveCalls.length).toBe(1));
    expect(saveCalls[0]!.defaultPath).toBe("поток-данных.mmd");
    expect(new TextDecoder().decode(saveCalls[0]!.bytes)).toBe(SOURCE);
    await waitFor(() => expect(screen.getByText("Сохранено")).toBeDefined());
  });

  test("отменённый диалог не выдаёт себя за сохранение", async () => {
    saveCalls.length = 0;
    saveResult = false;
    render(<VisualView visual={visual()} />);
    fireEvent.click(screen.getByText("Сохранить в файл"));

    await waitFor(() => expect(saveCalls.length).toBe(1));
    expect(screen.queryByText("Сохранено")).toBeNull();
  });

  test("заголовок без букв и цифр не даёт имя файла из одной точки", async () => {
    saveCalls.length = 0;
    saveResult = true;
    render(<VisualView visual={visual({ title: "— ??? —" })} />);
    fireEvent.click(screen.getByText("Сохранить в файл"));

    await waitFor(() => expect(saveCalls.length).toBe(1));
    expect(saveCalls[0]!.defaultPath).toBe("diagram.mmd");
  });

  test("без подписи блок подписи не рендерится", () => {
    render(<VisualView visual={visual({ caption: undefined })} />);
    expect(screen.queryByText("Слева направо")).toBeNull();
  });
});
