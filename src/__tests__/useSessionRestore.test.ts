import { beforeEach, describe, expect, mock, test } from "bun:test";
import { renderHook } from "@testing-library/react";
import { useSessionRestore } from "../hooks/useSessionRestore";

type Loaded = {
  openTabs: string[];
  activeTab: string | null;
  expandedDirs: string[];
};

function makeDeps(over: {
  ready?: boolean;
  loaded?: Loaded | null;
  docsRoot?: string | null;
  nodes?: unknown[];
  expandedSize?: number;
} = {}) {
  const loaded: Loaded | null =
    over.loaded === undefined
      ? { openTabs: ["a.adoc"], activeTab: "a.adoc", expandedDirs: [] }
      : over.loaded;
  return {
    project: { docsRoot: over.docsRoot === undefined ? "/repo/docs" : over.docsRoot },
    session: {
      ready: over.ready ?? true,
      loadedState: loaded,
      expandedDirs: new Set(Array.from({ length: over.expandedSize ?? 0 }, (_, i) => String(i))),
      syncPanelUi: mock(() => {}),
      seedShallowExpanded: mock(() => {}),
    },
    editor: { restoreTabs: mock(async () => {}) },
    tree: { nodes: over.nodes ?? [{ path: "sub", isDir: true, children: [] }] },
    layout: {
      hydrate: mock(() => {}),
      sidebarOpen: true,
      activeTool: null,
      bottomTool: null,
    },
  };
}

beforeEach(() => {});

describe("useSessionRestore", () => {
  test("restores tabs and layout once the session is ready", () => {
    const deps = makeDeps();
    renderHook(() => useSessionRestore(deps as never));

    expect(deps.editor.restoreTabs).toHaveBeenCalledWith(["a.adoc"], "a.adoc");
    expect(deps.layout.hydrate).toHaveBeenCalled();
  });

  test("nothing happens before the session is ready", () => {
    const deps = makeDeps({ ready: false });
    renderHook(() => useSessionRestore(deps as never));

    expect(deps.editor.restoreTabs).not.toHaveBeenCalled();
    expect(deps.layout.hydrate).not.toHaveBeenCalled();
    expect(deps.session.syncPanelUi).not.toHaveBeenCalled();
  });

  test("nothing happens without a docs root", () => {
    const deps = makeDeps({ docsRoot: null });
    renderHook(() => useSessionRestore(deps as never));
    expect(deps.editor.restoreTabs).not.toHaveBeenCalled();
  });

  test("hydrating does not immediately persist itself back", () => {
    // The persist effect watches the very values hydrate just set. Without
    // the suppression flag a restore would write itself back, and a project
    // would gradually overwrite its own saved layout.
    const deps = makeDeps();
    renderHook(() => useSessionRestore(deps as never));

    expect(deps.layout.hydrate).toHaveBeenCalled();
    expect(deps.session.syncPanelUi).not.toHaveBeenCalled();
  });

  test("a later layout change is persisted", () => {
    const deps = makeDeps();
    const { rerender } = renderHook(
      ({ bottomTool }) =>
        useSessionRestore({ ...deps, layout: { ...deps.layout, bottomTool } } as never),
      { initialProps: { bottomTool: null as string | null } },
    );
    expect(deps.session.syncPanelUi).not.toHaveBeenCalled();

    rerender({ bottomTool: "problems" });
    expect(deps.session.syncPanelUi).toHaveBeenCalledWith({
      sidebarOpen: true,
      rightTool: null,
      bottomTool: "problems",
    });
  });

  test("suppressNextPanelSync skips exactly one change", () => {
    const deps = makeDeps();
    const { result, rerender } = renderHook(
      ({ bottomTool }) =>
        useSessionRestore({ ...deps, layout: { ...deps.layout, bottomTool } } as never),
      { initialProps: { bottomTool: null as string | null } },
    );

    result.current.suppressNextPanelSync();
    rerender({ bottomTool: "problems" });
    expect(deps.session.syncPanelUi).not.toHaveBeenCalled();

    rerender({ bottomTool: "git" });
    expect(deps.session.syncPanelUi).toHaveBeenCalledTimes(1);
  });

  test("a first-open project gets its top level expanded", () => {
    const deps = makeDeps();
    renderHook(() => useSessionRestore(deps as never));
    expect(deps.session.seedShallowExpanded).toHaveBeenCalled();
  });

  test("a saved expansion set is left alone", () => {
    const deps = makeDeps({
      loaded: { openTabs: [], activeTab: null, expandedDirs: ["sub", "sub/deep"] },
    });
    renderHook(() => useSessionRestore(deps as never));
    expect(deps.session.seedShallowExpanded).not.toHaveBeenCalled();
  });

  test("an already-expanded tree is left alone", () => {
    const deps = makeDeps({ expandedSize: 3 });
    renderHook(() => useSessionRestore(deps as never));
    expect(deps.session.seedShallowExpanded).not.toHaveBeenCalled();
  });

  test("seeding waits for the tree and then happens once", () => {
    // `tree.nodes` arrives asynchronously, so the effect re-runs; the
    // per-project guard is what keeps it to a single seed.
    const deps = makeDeps({ nodes: [] });
    const { rerender } = renderHook(
      ({ nodes }) => useSessionRestore({ ...deps, tree: { nodes } } as never),
      { initialProps: { nodes: [] as unknown[] } },
    );
    expect(deps.session.seedShallowExpanded).not.toHaveBeenCalled();

    const loaded = [{ path: "sub", isDir: true, children: [] }];
    rerender({ nodes: loaded });
    rerender({ nodes: [...loaded] });
    expect(deps.session.seedShallowExpanded).toHaveBeenCalledTimes(1);
  });
});
