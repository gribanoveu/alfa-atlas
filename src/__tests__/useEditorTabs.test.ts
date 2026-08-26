import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualProject from "../lib/project";

let files: Record<string, string> = {};
let readThrows: string | null = null;
let writes: Array<[string, string]> = [];

mock.module("../lib/project", () => ({
  ...actualProject,
  readProjectFile: async (_docsRoot: string, path: string) => {
    if (readThrows) throw readThrows;
    const content = files[path];
    if (content === undefined) throw `no such file: ${path}`;
    return content;
  },
  writeProjectFile: async (_docsRoot: string, path: string, content: string) => {
    writes.push([path, content]);
    files[path] = content;
  },
}));

const { useEditorTabs } = await import("../hooks/useEditorTabs");

function render(docsRoot: string | null = "/repo/docs") {
  return renderHook(() => useEditorTabs(docsRoot));
}

beforeEach(() => {
  files = { "a.adoc": "= A", "b.adoc": "= B", "old/inner/deep.adoc": "= Deep" };
  readThrows = null;
  writes = [];
});

describe("useEditorTabs — opening and closing", () => {
  test("opening a file adds a tab and makes it active", async () => {
    const { result } = render();

    await act(async () => {
      await result.current.openFile("a.adoc");
    });

    expect(result.current.tabs.map((t) => t.path)).toEqual(["a.adoc"]);
    expect(result.current.activeTabId).toBe("a.adoc");
    expect(result.current.activeTab?.content).toBe("= A");
  });

  test("opening the same file again focuses the existing tab", async () => {
    const { result } = render();
    // Separate `act` calls: `result.current` only refreshes between them.
    await act(async () => {
      await result.current.openFile("a.adoc");
    });
    await act(async () => {
      await result.current.openFile("b.adoc");
    });
    await act(async () => {
      await result.current.openFile("a.adoc");
    });

    expect(result.current.tabs).toHaveLength(2);
    expect(result.current.activeTabId).toBe("a.adoc");
  });

  test("a missing file reports an error and opens nothing", async () => {
    const { result } = render();
    // `openFile` records the error *and* rethrows, so callers like
    // `openDiagnostic` can decide whether to surface it.
    await act(async () => {
      await expect(result.current.openFile("gone.adoc")).rejects.toBeDefined();
    });

    await waitFor(() => expect(result.current.error).not.toBeNull());
    expect(result.current.tabs).toHaveLength(0);
  });

  test("closing the active tab activates another", async () => {
    const { result } = render();
    await act(async () => {
      await result.current.openFile("a.adoc");
    });
    await act(async () => {
      await result.current.openFile("b.adoc");
    });

    await act(async () => {
      await result.current.closeTab("b.adoc");
    });

    expect(result.current.tabs.map((t) => t.path)).toEqual(["a.adoc"]);
    expect(result.current.activeTabId).toBe("a.adoc");
  });

  test("closing the last tab leaves nothing active", async () => {
    const { result } = render();
    await act(async () => {
      await result.current.openFile("a.adoc");
    });
    await act(async () => {
      await result.current.closeTab("a.adoc");
    });

    expect(result.current.tabs).toHaveLength(0);
    expect(result.current.activeTabId).toBeNull();
  });

  test("close-others keeps exactly the named tab", async () => {
    const { result } = render();
    await act(async () => {
      await result.current.openFile("a.adoc");
    });
    await act(async () => {
      await result.current.openFile("b.adoc");
    });

    await act(async () => {
      await result.current.closeOtherTabs("a.adoc");
    });

    expect(result.current.tabs.map((t) => t.path)).toEqual(["a.adoc"]);
  });
});

describe("useEditorTabs — editing and saving", () => {
  test("editing marks the tab dirty; saving writes and clears it", async () => {
    const { result } = render();
    await act(async () => {
      await result.current.openFile("a.adoc");
    });

    act(() => result.current.updateActiveContent("= A changed"));
    expect(result.current.activeTab?.dirty).toBe(true);

    await act(async () => {
      await result.current.saveActive();
    });

    expect(writes).toEqual([["a.adoc", "= A changed"]]);
    expect(result.current.activeTab?.dirty).toBe(false);
  });

  test("editing back to the saved content clears the dirty flag", async () => {
    // Otherwise a typo typed and undone would still prompt on close.
    const { result } = render();
    await act(async () => {
      await result.current.openFile("a.adoc");
    });

    act(() => result.current.updateActiveContent("= A changed"));
    act(() => result.current.updateActiveContent("= A"));
    expect(result.current.activeTab?.dirty).toBe(false);
  });

  test("saving with nothing dirty writes nothing", async () => {
    const { result } = render();
    await act(async () => {
      await result.current.openFile("a.adoc");
    });
    await act(async () => {
      await result.current.saveActive();
    });
    expect(writes).toEqual([]);
  });
});

describe("useEditorTabs — paths moving underneath", () => {
  test("a moved folder keeps the separator in its tabs' paths", async () => {
    // Regression: `newPath + slice(prefix.length)` dropped the separator, so
    // `old/inner/deep.adoc` became `new/placeinner/deep.adoc` — a path that
    // exists nowhere, leaving the tab unable to save or reload.
    const { result } = render();
    await act(async () => {
      await result.current.openFile("old/inner/deep.adoc");
    });

    act(() => result.current.remapTabsUnder("old", "new/place"));

    expect(result.current.tabs[0]?.path).toBe("new/place/inner/deep.adoc");
    expect(result.current.tabs[0]?.id).toBe("new/place/inner/deep.adoc");
  });

  test("a renamed file itself is remapped", async () => {
    const { result } = render();
    await act(async () => {
      await result.current.openFile("a.adoc");
    });

    act(() => result.current.remapTabsUnder("a.adoc", "renamed.adoc"));
    expect(result.current.tabs[0]?.path).toBe("renamed.adoc");
    expect(result.current.tabs[0]?.title).toBe("renamed.adoc");
  });

  test("a remap leaves tabs outside the moved folder alone", async () => {
    files["older-sibling.adoc"] = "= Sibling";
    const { result } = render();
    await act(async () => {
      await result.current.openFile("older-sibling.adoc");
    });

    act(() => result.current.remapTabsUnder("old", "new"));
    expect(result.current.tabs[0]?.path).toBe("older-sibling.adoc");
  });

  test("deleting a folder closes the tabs under it", async () => {
    const { result } = render();
    await act(async () => {
      await result.current.openFile("a.adoc");
    });
    await act(async () => {
      await result.current.openFile("old/inner/deep.adoc");
    });

    act(() => result.current.discardTabsUnder("old"));
    expect(result.current.tabs.map((t) => t.path)).toEqual(["a.adoc"]);
  });
});
