import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ToolCallLogFilter, ToolCallLogRow } from "../lib/toolCallLog";
import * as actualToolCallLog from "../lib/toolCallLog";

let queries: ToolCallLogFilter[] = [];
let total = 0;
let rowsFor: (filter: ToolCallLogFilter) => ToolCallLogRow[] = () => [];
let queryRejectsWith: string | null = null;
let clearCalls = 0;
let clearRejectsWith: string | null = null;

function row(id: number): ToolCallLogRow {
  return {
    id,
    tsMs: 1_700_000_000_000 + id,
    repoRoot: "/repo",
    source: "chat",
    round: 1,
    providerId: "anthropic",
    model: "opus",
    tool: "read_file",
    argsJson: {},
    status: "ok",
    errorMessage: null,
    resultJson: null,
    durationMs: 12,
  };
}

// The IPC wrappers are the seam this layer exists to provide: thin, typed,
// and the only thing between the hook and Tauri.
mock.module("../lib/toolCallLog", () => ({
  ...actualToolCallLog,
  queryToolCallLog: async (filter: ToolCallLogFilter) => {
    queries.push(filter);
    if (queryRejectsWith) throw queryRejectsWith;
    return { rows: rowsFor(filter), total };
  },
  clearToolCallLog: async () => {
    clearCalls += 1;
    if (clearRejectsWith) throw clearRejectsWith;
    return 0;
  },
}));

const { useToolCallLog, sinceMsFor, TOOL_CALL_LOG_PAGE_SIZE } = await import(
  "../hooks/useToolCallLog"
);

beforeEach(() => {
  queries = [];
  total = 0;
  rowsFor = () => [];
  queryRejectsWith = null;
  clearCalls = 0;
  clearRejectsWith = null;
});

describe("sinceMsFor", () => {
  test('"all" means no lower bound', () => {
    expect(sinceMsFor("all")).toBeUndefined();
  });

  test("the other ranges are offsets from now, ordered oldest-first", () => {
    const now = Date.now();
    const hour = sinceMsFor("hour");
    const day = sinceMsFor("day");
    const week = sinceMsFor("week");
    expect(week).toBeLessThan(day!);
    expect(day).toBeLessThan(hour!);
    // Within a second of the expected offset — the hook resolves against
    // "now" at query time, not at option-pick time.
    expect(Math.abs(hour! - (now - 60 * 60 * 1000))).toBeLessThan(1000);
  });
});

describe("useToolCallLog", () => {
  test("loads the first page on mount, scoped to the current repo", async () => {
    total = 2;
    rowsFor = () => [row(1), row(2)];
    const { result } = renderHook(() => useToolCallLog("/repo"));

    await waitFor(() => expect(result.current.rows).toHaveLength(2));
    expect(result.current.total).toBe(2);
    expect(result.current.loading).toBe(false);
    expect(queries[0]).toMatchObject({
      repoRoot: "/repo",
      limit: TOOL_CALL_LOG_PAGE_SIZE,
      offset: 0,
    });
  });

  test("unchecking the repo filter drops the repoRoot constraint", async () => {
    const { result } = renderHook(() => useToolCallLog("/repo"));
    await waitFor(() => expect(queries).toHaveLength(1));

    act(() => result.current.setOnlyCurrentRepo(false));
    await waitFor(() => expect(queries.length).toBeGreaterThan(1));
    expect(queries.at(-1)!.repoRoot).toBeUndefined();
  });

  test("a blank search is no constraint, a filled one is trimmed", async () => {
    const { result } = renderHook(() => useToolCallLog(null));
    await waitFor(() => expect(queries).toHaveLength(1));
    expect(queries[0].search).toBeUndefined();

    act(() => result.current.setSearch("  grep  "));
    await waitFor(() => expect(queries.at(-1)!.search).toBe("grep"));
  });

  test("paging moves the offset and stops at zero", async () => {
    total = 120;
    rowsFor = () => Array.from({ length: TOOL_CALL_LOG_PAGE_SIZE }, (_, i) => row(i));
    const { result } = renderHook(() => useToolCallLog("/repo"));
    await waitFor(() => expect(result.current.rows).toHaveLength(TOOL_CALL_LOG_PAGE_SIZE));

    expect(result.current.canPrev).toBe(false);
    expect(result.current.canNext).toBe(true);

    act(() => result.current.nextPage());
    await waitFor(() => expect(result.current.offset).toBe(TOOL_CALL_LOG_PAGE_SIZE));
    expect(result.current.canPrev).toBe(true);

    // Two steps back from page two would go negative without the clamp.
    act(() => result.current.prevPage());
    act(() => result.current.prevPage());
    await waitFor(() => expect(result.current.offset).toBe(0));
  });

  test("changing a filter resets to the first page", async () => {
    total = 200;
    rowsFor = () => Array.from({ length: TOOL_CALL_LOG_PAGE_SIZE }, (_, i) => row(i));
    const { result } = renderHook(() => useToolCallLog("/repo"));
    await waitFor(() => expect(result.current.rows).toHaveLength(TOOL_CALL_LOG_PAGE_SIZE));

    act(() => result.current.nextPage());
    await waitFor(() => expect(result.current.offset).toBe(TOOL_CALL_LOG_PAGE_SIZE));

    // Without the reset, narrowing could leave `offset` past the new total
    // and show an empty page over a non-empty result.
    act(() => result.current.setStatus("error"));
    await waitFor(() => expect(result.current.offset).toBe(0));
    expect(queries.at(-1)).toMatchObject({ status: "error", offset: 0 });
  });

  test("the range label reads as a human count", async () => {
    total = 214;
    rowsFor = () => Array.from({ length: TOOL_CALL_LOG_PAGE_SIZE }, (_, i) => row(i));
    const { result } = renderHook(() => useToolCallLog("/repo"));

    await waitFor(() => expect(result.current.rangeLabel).toBe("1–50 из 214"));
  });

  test("an empty log reads 0 из 0", async () => {
    const { result } = renderHook(() => useToolCallLog("/repo"));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.rangeLabel).toBe("0 из 0");
  });

  test("a failing query surfaces the message", async () => {
    queryRejectsWith = "db locked";
    const { result } = renderHook(() => useToolCallLog("/repo"));

    await waitFor(() => expect(result.current.error).toBe("db locked"));
    expect(result.current.loading).toBe(false);
  });

  test("clearing reloads and reports success", async () => {
    total = 3;
    rowsFor = () => [row(1), row(2), row(3)];
    const { result } = renderHook(() => useToolCallLog("/repo"));
    await waitFor(() => expect(result.current.rows).toHaveLength(3));

    total = 0;
    rowsFor = () => [];
    let ok: boolean | undefined;
    await act(async () => {
      ok = await result.current.clearLog();
    });

    expect(ok).toBe(true);
    expect(clearCalls).toBe(1);
    await waitFor(() => expect(result.current.rows).toHaveLength(0));
    expect(result.current.clearing).toBe(false);
  });

  test("a failing clear reports false and leaves the rows alone", async () => {
    total = 1;
    rowsFor = () => [row(1)];
    clearRejectsWith = "read-only";
    const { result } = renderHook(() => useToolCallLog("/repo"));
    await waitFor(() => expect(result.current.rows).toHaveLength(1));

    let ok: boolean | undefined;
    await act(async () => {
      ok = await result.current.clearLog();
    });

    expect(ok).toBe(false);
    expect(result.current.error).toBe("read-only");
    expect(result.current.rows).toHaveLength(1);
  });
});
