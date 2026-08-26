import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualWorkspace from "../lib/workspace";
import type { WorkspaceState } from "../lib/workspace";

const DEFAULTS: WorkspaceState = {
  openTabs: [],
  activeTab: null,
  expandedDirs: ["."],
  sidebarOpen: true,
  rightTool: null,
  bottomTool: null,
};

let stored: WorkspaceState;
let loadThrows: string | null = null;
let saves: Array<[string, WorkspaceState]> = [];

mock.module("../lib/workspace", () => ({
  ...actualWorkspace,
  DEFAULT_WORKSPACE_STATE: DEFAULTS,
  getWorkspaceState: async () => {
    if (loadThrows) throw loadThrows;
    return stored;
  },
  saveWorkspaceState: async (root: string, state: WorkspaceState) => {
    saves.push([root, state]);
  },
}));

const { useWorkspaceSession, collectDirPaths } = await import("../hooks/useWorkspaceSession");

/** The hook debounces writes by 200ms. */
async function flushPersist() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 260));
  });
}

function render(repoRoot: string | null = "/repo", docsRoot: string | null = "/repo/docs") {
  return renderHook(() => useWorkspaceSession(repoRoot, docsRoot));
}

beforeEach(() => {
  stored = { ...DEFAULTS };
  loadThrows = null;
  saves = [];
});

describe("useWorkspaceSession — loading", () => {
  test("loads the saved state and reports ready", async () => {
    stored = { ...DEFAULTS, openTabs: ["a.adoc"], expandedDirs: [".", "sub"] };
    const { result } = render();

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.loadedState?.openTabs).toEqual(["a.adoc"]);
    expect(result.current.expandedDirs.has("sub")).toBe(true);
  });

  test("no project means defaults, and still ready", async () => {
    const { result } = render(null, null);
    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(Array.from(result.current.expandedDirs)).toEqual(["."]);
    expect(result.current.loadedState).toBeNull();
  });

  test("a failing load does not leave the app un-ready", async () => {
    loadThrows = "workspace file corrupt";
    const { result } = render();
    await waitFor(() => expect(result.current.ready).toBe(true));
  });
});

describe("useWorkspaceSession — expansion", () => {
  test("toggling a folder opens and closes it", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.toggleDir("sub"));
    expect(result.current.expandedDirs.has("sub")).toBe(true);

    act(() => result.current.toggleDir("sub"));
    expect(result.current.expandedDirs.has("sub")).toBe(false);
  });

  test("the root stays expanded whatever else is toggled", async () => {
    // Collapsing the root would hide the whole tree with no way back.
    const { result } = render();
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.toggleDir("."));
    expect(result.current.expandedDirs.has(".")).toBe(true);
  });

  test("ensureExpanded opens every ancestor, not just the folder itself", async () => {
    // Revealing `a/b/c.adoc` is useless if `a` and `a/b` stay closed.
    const { result } = render();
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.ensureExpanded("a/b/c"));
    for (const p of [".", "a", "a/b", "a/b/c"]) {
      expect(result.current.expandedDirs.has(p)).toBe(true);
    }
  });

  test("expandAll and collapseAll bracket the tree", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.expandAll(["a", "a/b", "c"]));
    expect(result.current.expandedDirs.size).toBe(4);

    act(() => result.current.collapseAll());
    expect(Array.from(result.current.expandedDirs)).toEqual(["."]);
  });

  test("seeding opens the shallow levels only", async () => {
    // Two levels is enough to see the shape without unfolding everything.
    const { result } = render();
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.seedShallowExpanded(["a", "a/b", "a/b/deep"]));
    expect(result.current.expandedDirs.has("a")).toBe(true);
    expect(result.current.expandedDirs.has("a/b")).toBe(true);
    expect(result.current.expandedDirs.has("a/b/deep")).toBe(false);
  });

  test("seeding leaves an arrangement the user already has", async () => {
    stored = { ...DEFAULTS, expandedDirs: [".", "x", "y"] };
    const { result } = render();
    await waitFor(() => expect(result.current.expandedDirs.size).toBe(3));

    act(() => result.current.seedShallowExpanded(["a", "b"]));
    expect(result.current.expandedDirs.has("a")).toBe(false);
  });

  test("a moved folder keeps its expansion at the new path", async () => {
    stored = { ...DEFAULTS, expandedDirs: [".", "old", "old/inner"] };
    const { result } = render();
    await waitFor(() => expect(result.current.expandedDirs.size).toBe(3));

    act(() => result.current.remapExpandedUnder("old", "new/place"));

    expect(result.current.expandedDirs.has("new/place")).toBe(true);
    expect(result.current.expandedDirs.has("new/place/inner")).toBe(true);
    expect(result.current.expandedDirs.has("old")).toBe(false);
  });

  test("a remap leaves unrelated folders alone", async () => {
    stored = { ...DEFAULTS, expandedDirs: [".", "old", "older-sibling"] };
    const { result } = render();
    await waitFor(() => expect(result.current.expandedDirs.size).toBe(3));

    act(() => result.current.remapExpandedUnder("old", "new"));
    // A prefix match on the bare string would have swallowed this one.
    expect(result.current.expandedDirs.has("older-sibling")).toBe(true);
  });
});

describe("useWorkspaceSession — persistence", () => {
  // Writes are debounced by 200ms, so a timer armed by an earlier test can
  // still be in flight when this block starts. Let it land, then start
  // counting from zero.
  beforeEach(async () => {
    await act(async () => {
      await new Promise((r) => setTimeout(r, 260));
    });
    saves = [];
  });

  test("a change is written back, debounced", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.toggleDir("sub"));
    expect(saves).toHaveLength(0);

    await flushPersist();
    expect(saves.at(-1)?.[0]).toBe("/repo");
    expect(saves.at(-1)?.[1].expandedDirs).toContain("sub");
  });

  test("a burst of changes writes once", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => {
      result.current.toggleDir("a");
      result.current.toggleDir("b");
      result.current.toggleDir("c");
    });

    await flushPersist();
    expect(saves).toHaveLength(1);
  });

  test("tabs and panel state ride along in the same record", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.syncTabs(["a.adoc", "b.adoc"], "b.adoc"));
    act(() =>
      result.current.syncPanelUi({ sidebarOpen: false, rightTool: "git", bottomTool: null }),
    );

    await flushPersist();
    const written = saves.at(-1)?.[1];
    expect(written?.openTabs).toEqual(["a.adoc", "b.adoc"]);
    expect(written?.activeTab).toBe("b.adoc");
    expect(written?.sidebarOpen).toBe(false);
    expect(written?.rightTool).toBe("git");
  });

  test("nothing is written without a project", async () => {
    const { result } = render(null, null);
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => result.current.toggleDir("sub"));
    await flushPersist();
    expect(saves).toEqual([]);
  });
});

describe("collectDirPaths", () => {
  test("collects directories and skips files", () => {
    const nodes = [
      { path: "a", isDir: true, children: [{ path: "a/f.adoc", isDir: false }] },
      { path: "b.adoc", isDir: false },
    ];
    expect(collectDirPaths(nodes as never)).toEqual(["a"]);
  });

  test("descends into nested directories", () => {
    const nodes = [
      {
        path: "a",
        isDir: true,
        children: [{ path: "a/b", isDir: true, children: [{ path: "a/b/c", isDir: true }] }],
      },
    ];
    expect(collectDirPaths(nodes as never)).toEqual(["a", "a/b", "a/b/c"]);
  });

  test("an empty tree yields nothing", () => {
    expect(collectDirPaths([])).toEqual([]);
  });
});
