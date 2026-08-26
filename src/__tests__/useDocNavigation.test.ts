import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook } from "@testing-library/react";
import * as actualWorkspaceIndex from "../lib/workspaceIndex";

let anchors: Array<{ id: string; line: number; column: number }> = [];
let anchorsThrow: string | null = null;

mock.module("../lib/workspaceIndex", () => ({
  ...actualWorkspaceIndex,
  findAnchors: async () => {
    if (anchorsThrow) throw anchorsThrow;
    return anchors;
  },
}));

const { useDocNavigation } = await import("../hooks/useDocNavigation");

let openFileFails = false;

function makeDeps() {
  return {
    editor: {
      openFile: mock(async (p: string) => {
        if (openFileFails) throw new Error(`no such file: ${p}`);
      }),
      activeTab: { path: "guide.adoc" },
    },
    project: { repoRoot: "/repo", docsRoot: "/repo/docs" },
    layout: { bottomTool: "problems", setBottomToolId: mock(() => {}) },
    workspaceIndex: { diagnostics: [] as Array<Record<string, unknown>> },
    monacoInstance: null,
  };
}

function render(overrides: Partial<ReturnType<typeof makeDeps>> = {}) {
  const deps = { ...makeDeps(), ...overrides };
  return { deps, ...renderHook(() => useDocNavigation(deps as never)) };
}

beforeEach(() => {
  anchors = [];
  anchorsThrow = null;
  openFileFails = false;
});

describe("useDocNavigation", () => {
  test("opening a search hit asks the editor to scroll there", async () => {
    const { deps, result } = render();
    await act(async () => {
      await result.current.openDocsSearchHit("guide.adoc", 42);
    });

    expect(deps.editor.openFile).toHaveBeenCalledWith("guide.adoc");
    expect(result.current.revealRequest).toMatchObject({ line: 42, column: 1 });
  });

  test("the same hit twice produces two distinct requests", async () => {
    // The id exists precisely so a repeated click scrolls again — a prop
    // that compared equal would be ignored the second time.
    const { result } = render();
    await act(async () => {
      await result.current.openDocsSearchHit("guide.adoc", 42);
    });
    const first = result.current.revealRequest?.id;

    await act(async () => {
      await result.current.openDocsSearchHit("guide.adoc", 42);
    });

    expect(result.current.revealRequest?.id).not.toBe(first);
  });

  test("a missing file leaves no scroll request behind", async () => {
    openFileFails = true;
    const { result } = render();
    await act(async () => {
      await result.current.openDocsSearchHit("gone.adoc", 3);
    });
    expect(result.current.revealRequest).toBeNull();
  });

  test("clicking a diagnostic reveals the problems panel if it was collapsed", async () => {
    const { deps, result } = render({
      layout: { bottomTool: null, setBottomToolId: mock(() => {}) } as never,
    });
    await act(async () => {
      await result.current.openDiagnostic("docs/guide.adoc", 7, 2);
    });
    expect(deps.layout.setBottomToolId).toHaveBeenCalledWith("problems");
  });

  test("an already-open problems panel is left where it is", async () => {
    const { deps, result } = render();
    await act(async () => {
      await result.current.openDiagnostic("docs/guide.adoc", 7, 2);
    });
    expect(deps.layout.setBottomToolId).not.toHaveBeenCalled();
  });

  test("the reveal carries the diagnostic's own severity", async () => {
    const { result } = render({
      workspaceIndex: {
        diagnostics: [{ document: "docs/guide.adoc", line: 7, column: 2, severity: "warning" }],
      } as never,
    });
    await act(async () => {
      await result.current.openDiagnostic("docs/guide.adoc", 7, 2);
    });
    expect(result.current.revealRequest?.severity).toBe("warning");
  });

  test("a diagnostic with no matching entry is treated as an error", async () => {
    const { result } = render();
    await act(async () => {
      await result.current.openDiagnostic("docs/guide.adoc", 7, 2);
    });
    expect(result.current.revealRequest?.severity).toBe("error");
  });

  test("an anchored reference scrolls to the anchor's line", async () => {
    anchors = [{ id: "intro", line: 12, column: 1 }];
    const { result } = render();
    await act(async () => {
      await result.current.openDocumentReference("guide.adoc", "intro");
    });
    expect(result.current.revealRequest?.line).toBe(12);
  });

  test("an unknown anchor opens the file without scrolling", async () => {
    anchors = [{ id: "other", line: 12, column: 1 }];
    const { deps, result } = render();
    await act(async () => {
      await result.current.openDocumentReference("guide.adoc", "intro");
    });
    expect(deps.editor.openFile).toHaveBeenCalledWith("guide.adoc");
    expect(result.current.revealRequest).toBeNull();
  });

  test("an unavailable index leaves the user on the opened file", async () => {
    anchorsThrow = "index not built";
    const { deps, result } = render();
    await act(async () => {
      await result.current.openDocumentReference("guide.adoc", "intro");
    });
    expect(deps.editor.openFile).toHaveBeenCalled();
    expect(result.current.revealRequest).toBeNull();
  });

  test("external links are not our business", async () => {
    const { deps, result } = render();
    await act(async () => {
      await result.current.openXref("https://example.com/docs");
    });
    await act(async () => {
      await result.current.openXref("mailto:team@example.com");
    });
    expect(deps.editor.openFile).not.toHaveBeenCalled();
  });

  test("an xref with no active tab is ignored", async () => {
    const { deps, result } = render({
      editor: { openFile: mock(async () => {}), activeTab: null } as never,
    });
    await act(async () => {
      await result.current.openXref("other.adoc#intro");
    });
    expect(deps.editor.openFile).not.toHaveBeenCalled();
  });
});
