import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { MemoryLogDeleteRequest, MemoryLogFilter, MemoryLogRow } from "../lib/memoryLog";
import * as actualMemoryLog from "../lib/memoryLog";

let queries: MemoryLogFilter[] = [];
let total = 0;
let rowsFor: () => MemoryLogRow[] = () => [];
let projectStorePath: string | null = "/repo/.docflow/memory.json";
let queryRejectsWith: string | null = null;
let deletes: MemoryLogDeleteRequest[] = [];
let deleteRejectsWith: string | null = null;
let holdDelete: (() => void) | null = null;

function row(id: number, scope: "project" | "global" = "project"): MemoryLogRow {
  return { id, scope, date: "2026-08-27", text: `note ${id}`, storePath: "/store" };
}

mock.module("../lib/memoryLog", () => ({
  ...actualMemoryLog,
  queryMemoryLog: async (filter: MemoryLogFilter) => {
    queries.push(filter);
    if (queryRejectsWith) throw queryRejectsWith;
    return { rows: rowsFor(), total, projectStorePath, globalStorePath: "/home/global.json" };
  },
  deleteMemoryLogEntry: async (request: MemoryLogDeleteRequest) => {
    deletes.push(request);
    if (holdDelete) await new Promise<void>((resolve) => (holdDelete = resolve));
    if (deleteRejectsWith) throw deleteRejectsWith;
  },
}));

const { useMemoryLog, memoryRowKey, MEMORY_LOG_PAGE_SIZE } = await import("../hooks/useMemoryLog");

beforeEach(() => {
  queries = [];
  total = 0;
  rowsFor = () => [];
  projectStorePath = "/repo/.docflow/memory.json";
  queryRejectsWith = null;
  deletes = [];
  deleteRejectsWith = null;
  holdDelete = null;
});

describe("memoryRowKey", () => {
  test("scopes the id, since the two stores number entries independently", () => {
    expect(memoryRowKey({ scope: "project", id: 1 })).toBe("project-1");
    expect(memoryRowKey({ scope: "global", id: 1 })).toBe("global-1");
    expect(memoryRowKey({ scope: "project", id: 1 })).not.toBe(
      memoryRowKey({ scope: "global", id: 1 }),
    );
  });
});

describe("useMemoryLog", () => {
  test("loads the first page and both store paths", async () => {
    total = 2;
    rowsFor = () => [row(1), row(2, "global")];
    const { result } = renderHook(() => useMemoryLog("/repo"));

    await waitFor(() => expect(result.current.rows).toHaveLength(2));
    expect(result.current.total).toBe(2);
    expect(result.current.projectStorePath).toBe("/repo/.docflow/memory.json");
    expect(result.current.globalStorePath).toBe("/home/global.json");
    expect(queries[0]).toMatchObject({
      repoRoot: "/repo",
      limit: MEMORY_LOG_PAGE_SIZE,
      offset: 0,
    });
  });

  test("no open project means no repo constraint", async () => {
    renderHook(() => useMemoryLog(null));
    await waitFor(() => expect(queries).toHaveLength(1));
    expect(queries[0].repoRoot).toBeUndefined();
  });

  test("a blank filter is no constraint, a filled one is trimmed", async () => {
    const { result } = renderHook(() => useMemoryLog("/repo"));
    await waitFor(() => expect(queries).toHaveLength(1));
    expect(queries[0].search).toBeUndefined();
    expect(queries[0].scope).toBeUndefined();

    act(() => result.current.setSearch("  release  "));
    await waitFor(() => expect(queries.at(-1)!.search).toBe("release"));

    act(() => result.current.setScope("global"));
    await waitFor(() => expect(queries.at(-1)!.scope).toBe("global"));
  });

  test("changing a filter resets to the first page", async () => {
    total = 200;
    rowsFor = () => Array.from({ length: MEMORY_LOG_PAGE_SIZE }, (_, i) => row(i));
    const { result } = renderHook(() => useMemoryLog("/repo"));
    await waitFor(() => expect(result.current.rows).toHaveLength(MEMORY_LOG_PAGE_SIZE));

    act(() => result.current.nextPage());
    await waitFor(() => expect(result.current.offset).toBe(MEMORY_LOG_PAGE_SIZE));
    expect(result.current.canPrev).toBe(true);

    act(() => result.current.setScope("project"));
    await waitFor(() => expect(result.current.offset).toBe(0));
    expect(result.current.canPrev).toBe(false);
  });

  test("prevPage clamps at zero", async () => {
    const { result } = renderHook(() => useMemoryLog("/repo"));
    await waitFor(() => expect(result.current.loading).toBe(false));

    act(() => result.current.prevPage());
    expect(result.current.offset).toBe(0);
  });

  test("the range label reads as a human count", async () => {
    total = 7;
    rowsFor = () => [row(1), row(2), row(3)];
    const { result } = renderHook(() => useMemoryLog("/repo"));

    await waitFor(() => expect(result.current.rangeLabel).toBe("1–3 из 7"));
  });

  test("a failing query surfaces the message", async () => {
    queryRejectsWith = "store corrupt";
    const { result } = renderHook(() => useMemoryLog("/repo"));

    await waitFor(() => expect(result.current.error).toBe("store corrupt"));
    expect(result.current.loading).toBe(false);
  });

  test("deleting a project entry passes the repo, a global one does not", async () => {
    total = 2;
    rowsFor = () => [row(1), row(2, "global")];
    const { result } = renderHook(() => useMemoryLog("/repo"));
    await waitFor(() => expect(result.current.rows).toHaveLength(2));

    await act(async () => {
      await result.current.deleteEntry(row(1));
    });
    await act(async () => {
      await result.current.deleteEntry(row(2, "global"));
    });

    // Only a project-scoped entry needs a repo to resolve its store.
    expect(deletes).toEqual([
      { scope: "project", id: 1, repoRoot: "/repo" },
      { scope: "global", id: 2, repoRoot: undefined },
    ]);
  });

  test("a successful delete reloads and clears any earlier error", async () => {
    queryRejectsWith = "transient";
    const { result } = renderHook(() => useMemoryLog("/repo"));
    await waitFor(() => expect(result.current.error).toBe("transient"));

    queryRejectsWith = null;
    total = 1;
    rowsFor = () => [row(9)];

    let ok: boolean | undefined;
    await act(async () => {
      ok = await result.current.deleteEntry(row(1));
    });

    expect(ok).toBe(true);
    await waitFor(() => expect(result.current.error).toBeNull());
    expect(result.current.rows).toHaveLength(1);
    expect(result.current.deletingKey).toBeNull();
  });

  test("a failing delete reports false and surfaces the message", async () => {
    total = 1;
    rowsFor = () => [row(1)];
    deleteRejectsWith = "read-only store";
    const { result } = renderHook(() => useMemoryLog("/repo"));
    await waitFor(() => expect(result.current.rows).toHaveLength(1));

    let ok: boolean | undefined;
    await act(async () => {
      ok = await result.current.deleteEntry(row(1));
    });

    expect(ok).toBe(false);
    expect(result.current.error).toBe("read-only store");
    expect(result.current.deletingKey).toBeNull();
  });

  test("a repeated click on a row already being deleted is ignored", async () => {
    total = 1;
    rowsFor = () => [row(1)];
    const { result } = renderHook(() => useMemoryLog("/repo"));
    await waitFor(() => expect(result.current.rows).toHaveLength(1));

    // Hold the first delete open so the second click lands mid-flight.
    holdDelete = () => {};
    let first: Promise<boolean> | undefined;
    act(() => {
      first = result.current.deleteEntry(row(1));
    });
    await waitFor(() => expect(result.current.deletingKey).toBe("project-1"));

    let second: boolean | undefined;
    await act(async () => {
      second = await result.current.deleteEntry(row(1));
    });
    expect(second).toBe(false);
    expect(deletes).toHaveLength(1);

    const release = holdDelete!;
    await act(async () => {
      release();
      await first;
    });
    expect(result.current.deletingKey).toBeNull();
  });
});
