import { describe, expect, mock, test } from "bun:test";
import { act, renderHook } from "@testing-library/react";
import { useEditorTabActions } from "../hooks/useEditorTabActions";
import type { Visual } from "../lib/visuals";

type EditorStub = ReturnType<typeof makeEditor>;

function makeEditor(tabs: { id: string; title: string; dirty: boolean }[] = []) {
  return {
    tabs,
    activeTabId: tabs[0]?.id ?? null,
    selectTab: mock(() => {}),
    closeTab: mock(async () => {}),
    closeAllTabs: mock(async () => {}),
    closeOtherTabs: mock(async () => {}),
  };
}

function render(
  editor: EditorStub,
  title: string | null = "Мой API",
  repoRoot: string | null = "/repo/a",
) {
  return renderHook(
    (props: { repoRoot: string | null }) =>
      useEditorTabActions({
        // The hook only reads these few members; the real hooks are far
        // larger and none of the rest is reachable from here.
        editor: editor as never,
        specsRepo: { info: title ? ({ title } as never) : null },
        repoRoot: props.repoRoot,
      }),
    { initialProps: { repoRoot } },
  );
}

function visual(id: string, title: string, source = "flowchart TD"): Visual {
  return { id, title, content: { kind: "diagram", format: "mermaid", source } };
}

describe("useEditorTabActions", () => {
  test("switching projects closes the tabs that belong to the old repository", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result, rerender } = render(editor);

    act(() => result.current.openArtifactTab("art-1"));
    act(() => result.current.openVisualTab(visual("v1", "Схема")));
    act(() => result.current.openApiExplorerTab());
    act(() => result.current.openUtilityTab("unixtime"));
    expect(result.current.displayTabs.map((t) => t.id)).toEqual([
      "a.adoc",
      "openapi",
      "utility:unixtime",
      "artifact:art-1",
      "visual:v1",
    ]);

    rerender({ repoRoot: "/repo/b" });

    // Артефакт хранится по репозиторию, визуализация пришла из чата прошлого
    // проекта, Explorer описывает прошлую спеку — все три закрываются.
    // Конвертер к проекту не привязан и остаётся.
    expect(result.current.displayTabs.map((t) => t.id)).toEqual([
      "a.adoc",
      "utility:unixtime",
    ]);
    expect(result.current.activeKind).toBe("file");
  });


  test("the strip shows file tabs only until the API Explorer is opened", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    expect(result.current.displayTabs.map((t) => t.id)).toEqual(["a.adoc"]);

    act(() => result.current.openApiExplorerTab());
    expect(result.current.displayTabs.map((t) => t.id)).toEqual(["a.adoc", "openapi"]);
    expect(result.current.activeKind).toBe("openapi");
  });

  test("the API Explorer tab takes its title from the specs repo", () => {
    const { result } = render(makeEditor(), "Платежи API");
    act(() => result.current.openApiExplorerTab());
    expect(result.current.displayTabs[0]?.title).toBe("Платежи API");
  });

  test("it falls back to a generic title when the specs repo has none", () => {
    const { result } = render(makeEditor(), null);
    act(() => result.current.openApiExplorerTab());
    expect(result.current.displayTabs[0]?.title).toBe("API Explorer");
  });

  test("selecting the explorer does not touch the file editor", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.selectTab("openapi"));
    expect(result.current.activeKind).toBe("openapi");
    expect(editor.selectTab).not.toHaveBeenCalled();
  });

  test("selecting a file tab delegates to the editor", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.selectTab("a.adoc"));
    expect(editor.selectTab).toHaveBeenCalledWith("a.adoc");
  });

  test("closing the explorer removes it and hands focus back to files", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.openApiExplorerTab());

    act(() => result.current.closeTab("openapi"));
    expect(result.current.displayTabs.map((t) => t.id)).toEqual(["a.adoc"]);
    expect(result.current.activeKind).toBe("file");
    expect(editor.closeTab).not.toHaveBeenCalled();
  });

  test("close-all clears the explorer too, not just the file tabs", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.openApiExplorerTab());

    act(() => result.current.closeAllTabs());
    // The file tabs are the editor's to drop (asserted via the delegate);
    // what this hook owns is the explorer disappearing with them.
    expect(result.current.openApiTabOpen).toBe(false);
    expect(result.current.activeKind).toBe("file");
    expect(editor.closeAllTabs).toHaveBeenCalled();
  });

  test("close-others from the explorer keeps the explorer and drops every file", () => {
    // `editor.closeOtherTabs` cannot express this: it does not know the
    // explorer exists, so keeping it means closing all files instead.
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.openApiExplorerTab());

    act(() => result.current.closeOtherTabs("openapi"));
    expect(editor.closeAllTabs).toHaveBeenCalled();
    expect(editor.closeOtherTabs).not.toHaveBeenCalled();
    expect(result.current.activeKind).toBe("openapi");
    expect(result.current.openApiTabOpen).toBe(true);
  });

  test("close-others from a file tab drops the explorer and delegates the rest", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.openApiExplorerTab());

    act(() => result.current.closeOtherTabs("a.adoc"));
    expect(result.current.openApiTabOpen).toBe(false);
    expect(editor.closeOtherTabs).toHaveBeenCalledWith("a.adoc");
  });

  test("opening a utility appends its tab and makes it active", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    expect(result.current.displayTabs.map((t) => t.id)).toEqual(["a.adoc"]);

    act(() => result.current.openUtilityTab("unixtime"));
    expect(result.current.displayTabs.map((t) => t.id)).toEqual([
      "a.adoc",
      "utility:unixtime",
    ]);
    expect(result.current.displayTabs[1]?.title).toBe("Конвертер Unixtime");
    expect(result.current.activeKind).toBe("utility");
    expect(result.current.activeUtility).toBe("unixtime");
  });

  test("reopening an already-open utility focuses it instead of duplicating", () => {
    const { result } = render(makeEditor());
    act(() => result.current.openUtilityTab("unixtime"));
    act(() => result.current.selectTab("openapi"));
    act(() => result.current.openUtilityTab("unixtime"));

    expect(result.current.displayTabs.filter((t) => t.id === "utility:unixtime")).toHaveLength(1);
    expect(result.current.activeKind).toBe("utility");
  });

  test("selecting a utility tab does not touch the file editor", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.openUtilityTab("unixtime"));
    act(() => result.current.selectTab("utility:unixtime"));

    expect(result.current.activeKind).toBe("utility");
    expect(editor.selectTab).not.toHaveBeenCalled();
  });

  test("closing the active utility removes it and hands focus back to files", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.openUtilityTab("unixtime"));

    act(() => result.current.closeTab("utility:unixtime"));
    expect(result.current.displayTabs.map((t) => t.id)).toEqual(["a.adoc"]);
    expect(result.current.activeKind).toBe("file");
    expect(result.current.activeUtility).toBeNull();
    expect(editor.closeTab).not.toHaveBeenCalled();
  });

  test("close-all clears utility tabs along with the files", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.openUtilityTab("unixtime"));

    act(() => result.current.closeAllTabs());
    expect(result.current.displayTabs.map((t) => t.id)).toEqual(["a.adoc"]);
    expect(result.current.activeKind).toBe("file");
    expect(editor.closeAllTabs).toHaveBeenCalled();
  });

  test("close-others from a utility keeps it and drops every file", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.openApiExplorerTab());
    act(() => result.current.openUtilityTab("unixtime"));

    act(() => result.current.closeOtherTabs("utility:unixtime"));
    expect(editor.closeAllTabs).toHaveBeenCalled();
    expect(editor.closeOtherTabs).not.toHaveBeenCalled();
    expect(result.current.openApiTabOpen).toBe(false);
    expect(result.current.activeKind).toBe("utility");
    expect(result.current.activeUtility).toBe("unixtime");
  });

  test("close-others from a file tab drops the utility tabs too", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.openUtilityTab("unixtime"));

    act(() => result.current.closeOtherTabs("a.adoc"));
    expect(result.current.displayTabs.map((t) => t.id)).toEqual(["a.adoc"]);
    expect(result.current.activeUtility).toBeNull();
    expect(editor.closeOtherTabs).toHaveBeenCalledWith("a.adoc");
  });

  test("a visualization opens as its own tab and carries its title", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);

    act(() => result.current.openVisualTab(visual("v1", "Поток данных")));
    expect(result.current.activeKind).toBe("visual");
    expect(result.current.displayTabs.map((t) => t.id)).toEqual(["a.adoc", "visual:v1"]);
    expect(result.current.displayTabs[1]?.title).toBe("Поток данных");
    // Read-only tab: nothing can make it dirty.
    expect(result.current.displayTabs[1]?.dirty).toBe(false);
    expect(result.current.activeVisual?.content.source).toBe("flowchart TD");
  });

  test("reopening the same id replaces the payload instead of duplicating the tab", () => {
    // The assistant redrawing a diagram must update the open tab, not be
    // silently ignored and not stack a second tab with the same id.
    const { result } = render(makeEditor());
    act(() => result.current.openVisualTab(visual("v1", "Первый", "flowchart TD")));
    act(() => result.current.openVisualTab(visual("v1", "Второй", "sequenceDiagram")));

    expect(result.current.displayTabs.map((t) => t.id)).toEqual(["visual:v1"]);
    expect(result.current.displayTabs[0]?.title).toBe("Второй");
    expect(result.current.activeVisual?.content.source).toBe("sequenceDiagram");
  });

  test("selecting a visualization tab does not touch the file editor", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.openVisualTab(visual("v1", "Схема")));
    act(() => result.current.selectTab("a.adoc"));
    act(() => result.current.selectTab("visual:v1"));

    expect(result.current.activeKind).toBe("visual");
    expect(result.current.activeVisual?.id).toBe("v1");
    expect(editor.selectTab).toHaveBeenCalledTimes(1);
    expect(editor.selectTab).toHaveBeenCalledWith("a.adoc");
  });

  test("switching back to the already-active file tab leaves the visualization", () => {
    // The neighbour left of a visualization tab is usually the file the user
    // came from, so it is still `editor.activeTabId` and `editor.selectTab`
    // is a no-op on it. The pane kind must flip anyway, or that tab looks
    // dead to the click.
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.openVisualTab(visual("v1", "Схема")));
    expect(result.current.activeKind).toBe("visual");

    act(() => result.current.selectTab("a.adoc"));
    expect(result.current.activeKind).toBe("file");
  });

  test("closing the active visualization hands focus back to the file view", () => {
    const { result } = render(makeEditor());
    act(() => result.current.openVisualTab(visual("v1", "Схема")));
    act(() => result.current.closeTab("visual:v1"));

    expect(result.current.displayTabs).toEqual([]);
    expect(result.current.activeVisual).toBeNull();
    expect(result.current.activeKind).toBe("file");
  });

  test("closing an inactive visualization leaves the active one alone", () => {
    const { result } = render(makeEditor());
    act(() => result.current.openVisualTab(visual("v1", "Первая")));
    act(() => result.current.openVisualTab(visual("v2", "Вторая")));
    act(() => result.current.closeTab("visual:v1"));

    expect(result.current.displayTabs.map((t) => t.id)).toEqual(["visual:v2"]);
    expect(result.current.activeVisual?.id).toBe("v2");
    expect(result.current.activeKind).toBe("visual");
  });

  test("\"close others\" from a visualization keeps it and closes everything else", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.openUtilityTab("unixtime" as never));
    act(() => result.current.openVisualTab(visual("v1", "Первая")));
    act(() => result.current.openVisualTab(visual("v2", "Вторая")));
    act(() => result.current.closeOtherTabs("visual:v1"));

    // File tabs are the stub editor's to drop (it only records the call),
    // so assert on the pseudo-tabs this hook actually owns.
    expect(result.current.displayTabs.map((t) => t.id)).toEqual(["a.adoc", "visual:v1"]);
    expect(result.current.activeVisual?.id).toBe("v1");
    expect(result.current.activeKind).toBe("visual");
    expect(editor.closeAllTabs).toHaveBeenCalled();
  });

  test("\"close others\" from a file tab drops open visualizations too", () => {
    const editor = makeEditor([{ id: "a.adoc", title: "a", dirty: false }]);
    const { result } = render(editor);
    act(() => result.current.openVisualTab(visual("v1", "Схема")));
    act(() => result.current.closeOtherTabs("a.adoc"));

    expect(result.current.activeVisual).toBeNull();
    expect(editor.closeOtherTabs).toHaveBeenCalledWith("a.adoc");
  });

  test("closing all tabs clears open visualizations", () => {
    const { result } = render(makeEditor());
    act(() => result.current.openVisualTab(visual("v1", "Схема")));
    act(() => result.current.closeAllTabs());

    expect(result.current.displayTabs).toEqual([]);
    expect(result.current.activeVisual).toBeNull();
    expect(result.current.activeKind).toBe("file");
  });

  test("undo and redo are inert until an editor instance is attached", () => {
    const { result } = render(makeEditor());
    // No instance yet — must not throw, which is the whole point of the ref
    // being nullable.
    expect(() => result.current.undo()).not.toThrow();
    expect(() => result.current.redo()).not.toThrow();
  });

  test("undo and redo drive the attached instance and restore focus", () => {
    const { result } = render(makeEditor());
    const trigger = mock(() => {});
    const focus = mock(() => {});
    act(() => result.current.onEditorInstanceChange({ trigger, focus } as never));

    act(() => result.current.undo());
    expect(trigger).toHaveBeenCalledWith("menu", "undo", null);
    act(() => result.current.redo());
    expect(trigger).toHaveBeenLastCalledWith("menu", "redo", null);
    expect(focus).toHaveBeenCalledTimes(2);
  });
});
