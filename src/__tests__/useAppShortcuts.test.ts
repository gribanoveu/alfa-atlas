import { beforeEach, describe, expect, mock, test } from "bun:test";
import { renderHook } from "@testing-library/react";
import { useAppShortcuts } from "../hooks/useAppShortcuts";

let saveResult = true;

function makeDeps(hasProject = true, tabs: { dirty: boolean }[] = []) {
  return {
    hasProject,
    editor: {
      tabs,
      saveActive: mock(async () => saveResult),
      goBack: mock(async () => {}),
      goForward: mock(async () => {}),
    },
    git: { scheduleRefresh: mock(() => {}) },
    openDocsSearch: mock(() => {}),
  };
}

function press(init: Partial<KeyboardEventInit> & { key: string }) {
  const event = new KeyboardEvent("keydown", { cancelable: true, ...init });
  window.dispatchEvent(event);
  return event;
}

function render(deps: ReturnType<typeof makeDeps>) {
  return renderHook(() => useAppShortcuts(deps as never));
}

beforeEach(() => {
  saveResult = true;
});

describe("useAppShortcuts — keys", () => {
  test("Ctrl+S saves and refreshes git", async () => {
    const deps = makeDeps();
    render(deps);

    const event = press({ key: "s", ctrlKey: true });
    expect(event.defaultPrevented).toBe(true);
    expect(deps.editor.saveActive).toHaveBeenCalled();
    await Promise.resolve();
    expect(deps.git.scheduleRefresh).toHaveBeenCalled();
  });

  test("a save that did not land leaves git alone", async () => {
    saveResult = false;
    const deps = makeDeps();
    render(deps);

    press({ key: "s", ctrlKey: true });
    await Promise.resolve();
    expect(deps.git.scheduleRefresh).not.toHaveBeenCalled();
  });

  test("Cmd+S works the same as Ctrl+S", () => {
    const deps = makeDeps();
    render(deps);
    press({ key: "s", metaKey: true });
    expect(deps.editor.saveActive).toHaveBeenCalled();
  });

  test("plain S is left to the editor", () => {
    const deps = makeDeps();
    render(deps);
    const event = press({ key: "s" });
    expect(event.defaultPrevented).toBe(false);
    expect(deps.editor.saveActive).not.toHaveBeenCalled();
  });

  test("Ctrl+S without a project still swallows the browser's own save", () => {
    const deps = makeDeps(false);
    render(deps);
    const event = press({ key: "s", ctrlKey: true });
    expect(event.defaultPrevented).toBe(true);
    expect(deps.editor.saveActive).not.toHaveBeenCalled();
  });

  test("Ctrl+Shift+F opens the docs search", () => {
    const deps = makeDeps();
    render(deps);
    press({ key: "f", ctrlKey: true, shiftKey: true });
    expect(deps.openDocsSearch).toHaveBeenCalled();
  });

  test("Ctrl+F without Shift is not ours", () => {
    const deps = makeDeps();
    render(deps);
    press({ key: "f", ctrlKey: true });
    expect(deps.openDocsSearch).not.toHaveBeenCalled();
  });

  test("Ctrl+Alt+Arrows navigate history", () => {
    const deps = makeDeps();
    render(deps);
    press({ key: "ArrowLeft", ctrlKey: true, altKey: true });
    expect(deps.editor.goBack).toHaveBeenCalled();
    press({ key: "ArrowRight", ctrlKey: true, altKey: true });
    expect(deps.editor.goForward).toHaveBeenCalled();
  });

  test("arrows without Alt are left alone", () => {
    const deps = makeDeps();
    render(deps);
    press({ key: "ArrowLeft", ctrlKey: true });
    expect(deps.editor.goBack).not.toHaveBeenCalled();
  });

  test("the listener is removed on unmount", () => {
    const deps = makeDeps();
    const { unmount } = render(deps);
    unmount();
    press({ key: "s", ctrlKey: true });
    expect(deps.editor.saveActive).not.toHaveBeenCalled();
  });
});

describe("useAppShortcuts — git refresh after save", () => {
  test("a dropping dirty count refreshes git", () => {
    const deps = makeDeps(true, [{ dirty: true }, { dirty: true }]);
    const { rerender } = renderHook(({ tabs }) => useAppShortcuts({ ...deps, editor: { ...deps.editor, tabs } } as never), {
      initialProps: { tabs: [{ dirty: true }, { dirty: true }] },
    });
    expect(deps.git.scheduleRefresh).not.toHaveBeenCalled();

    rerender({ tabs: [{ dirty: false }, { dirty: false }] });
    expect(deps.git.scheduleRefresh).toHaveBeenCalled();
  });

  test("a rising dirty count does not — nothing reached disk yet", () => {
    const deps = makeDeps();
    const { rerender } = renderHook(({ tabs }) => useAppShortcuts({ ...deps, editor: { ...deps.editor, tabs } } as never), {
      initialProps: { tabs: [] as { dirty: boolean }[] },
    });

    rerender({ tabs: [{ dirty: true }] });
    expect(deps.git.scheduleRefresh).not.toHaveBeenCalled();
  });
});
