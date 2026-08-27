import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualIndex from "../lib/workspaceIndex";
import type { Diagnostic, IndexEvent, IndexStats } from "../lib/workspaceIndex";

let listeners: Array<(e: { payload: IndexEvent }) => void> = [];
let diagnostics: Diagnostic[] = [];
let buildThrows: string | null = null;
let builds: string[] = [];
let clears = 0;

mock.module("@tauri-apps/api/event", () => ({
  listen: async (_channel: string, cb: (e: { payload: IndexEvent }) => void) => {
    listeners.push(cb);
    return () => {
      listeners = listeners.filter((l) => l !== cb);
    };
  },
}));
mock.module("../lib/workspaceIndex", () => ({
  ...actualIndex,
  buildIndex: async (root: string) => {
    builds.push(root);
    if (buildThrows) throw buildThrows;
  },
  clearIndex: async () => {
    clears += 1;
  },
  getDiagnostics: async () => diagnostics,
}));

const { useWorkspaceIndex } = await import("../hooks/useWorkspaceIndex");

function stats(over: Partial<IndexStats> = {}): IndexStats {
  return { documents: 1, anchors: 0, attributes: 0, images: 0, warnings: 0, errors: 0, ...over } as IndexStats;
}

async function emit(event: IndexEvent) {
  await act(async () => {
    for (const l of [...listeners]) l({ payload: event });
    await Promise.resolve();
  });
}

beforeEach(() => {
  listeners = [];
  diagnostics = [];
  buildThrows = null;
  builds = [];
  clears = 0;
});

describe("useWorkspaceIndex", () => {
  test("opening a repo starts a build", async () => {
    const { result } = renderHook(() => useWorkspaceIndex("/repo"));
    await waitFor(() => expect(builds).toEqual(["/repo"]));
    expect(result.current.status).toBe("building");
  });

  test("no repo means idle and no build", async () => {
    const { result } = renderHook(() => useWorkspaceIndex(null));
    await waitFor(() => expect(result.current.status).toBe("idle"));
    expect(builds).toEqual([]);
  });

  test("an inactive index is not built", async () => {
    const { result } = renderHook(() => useWorkspaceIndex("/repo", { active: false }));
    await waitFor(() => expect(result.current.status).toBe("idle"));
    expect(builds).toEqual([]);
  });

  test("progress events drive the status bar", async () => {
    const { result } = renderHook(() => useWorkspaceIndex("/repo"));
    await waitFor(() => expect(listeners).toHaveLength(1));

    await emit({ kind: "indexBuildingProgress", payload: { done: 3, total: 10, current: "a.adoc" } } as IndexEvent);

    expect(result.current.status).toBe("building");
    expect(result.current.progress).toEqual({ done: 3, total: 10, current: "a.adoc" });
  });

  test("a clean build ends as ready with the progress cleared", async () => {
    const { result } = renderHook(() => useWorkspaceIndex("/repo"));
    await waitFor(() => expect(listeners).toHaveLength(1));

    await emit({ kind: "indexBuildingFinished", payload: { stats: stats() } } as IndexEvent);

    expect(result.current.status).toBe("ready");
    expect(result.current.progress).toBeNull();
    expect(result.current.stats?.documents).toBe(1);
  });

  test("document problems end as warning, not error", async () => {
    // A missing include is a documentation issue, not a failed build — the
    // status bar must not claim the index broke.
    const { result } = renderHook(() => useWorkspaceIndex("/repo"));
    await waitFor(() => expect(listeners).toHaveLength(1));

    await emit({ kind: "indexBuildingFinished", payload: { stats: stats({ warnings: 2 }) } } as IndexEvent);
    expect(result.current.status).toBe("warning");

    await emit({ kind: "indexBuildingFinished", payload: { stats: stats({ errors: 1 }) } } as IndexEvent);
    expect(result.current.status).toBe("warning");
  });

  test("only a failing build is an error", async () => {
    buildThrows = "index root unreadable";
    const { result } = renderHook(() => useWorkspaceIndex("/repo"));

    await waitFor(() => expect(result.current.status).toBe("error"));
    expect(result.current.error).toBe("index root unreadable");
  });

  test("a finished build pulls the diagnostics", async () => {
    diagnostics = [{ document: "a.adoc", line: 1, column: 1, severity: "warning", message: "m" } as Diagnostic];
    const { result } = renderHook(() => useWorkspaceIndex("/repo"));
    await waitFor(() => expect(listeners).toHaveLength(1));

    await emit({ kind: "indexBuildingFinished", payload: { stats: stats({ warnings: 1 }) } } as IndexEvent);
    await waitFor(() => expect(result.current.diagnostics).toHaveLength(1));
  });

  test("a single document changing refreshes diagnostics without a rebuild", async () => {
    const { result } = renderHook(() => useWorkspaceIndex("/repo"));
    await waitFor(() => expect(builds).toHaveLength(1));

    diagnostics = [{ document: "a.adoc", line: 2, column: 1, severity: "error", message: "m" } as Diagnostic];
    await emit({ kind: "indexUpdated", payload: { document: "a.adoc" } } as IndexEvent);

    await waitFor(() => expect(result.current.diagnostics).toHaveLength(1));
    expect(builds).toHaveLength(1);
  });

  test("closing the project clears the backend index", async () => {
    const { rerender } = renderHook(({ root }) => useWorkspaceIndex(root), {
      initialProps: { root: "/repo" as string | null },
    });
    await waitFor(() => expect(builds).toHaveLength(1));

    rerender({ root: null });
    await waitFor(() => expect(clears).toBe(1));
  });

  test("rebuild restarts the scan and clears a previous error", async () => {
    buildThrows = "index root unreadable";
    const { result } = renderHook(() => useWorkspaceIndex("/repo"));
    await waitFor(() => expect(result.current.status).toBe("error"));

    buildThrows = null;
    await act(async () => {
      await result.current.rebuildIndex();
    });

    expect(result.current.error).toBeNull();
    expect(result.current.status).toBe("building");
    expect(builds).toHaveLength(2);
  });
});
