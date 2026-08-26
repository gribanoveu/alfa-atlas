import { describe, expect, mock, test } from "bun:test";
import { act, renderHook } from "@testing-library/react";
import { useEditorTabActions } from "../hooks/useEditorTabActions";

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

function render(editor: EditorStub, title: string | null = "Мой API") {
  return renderHook(() =>
    useEditorTabActions({
      // The hook only reads these few members; the real hooks are far
      // larger and none of the rest is reachable from here.
      editor: editor as never,
      specsRepo: { info: title ? ({ title } as never) : null },
    }),
  );
}

describe("useEditorTabActions", () => {
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
