import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { PlanRecord, PlanSummary } from "../lib/plans";
import * as actualPlans from "../lib/plans";

let lists: PlanSummary[][] = [];
let listCalls = 0;
let listRejectsWith: string | null = null;
let getCalls: string[] = [];
let getRejectsFor: string | null = null;
let deleted: string[] = [];
let deleteRejectsWith: string | null = null;

function summary(id: string): PlanSummary {
  return {
    id,
    name: `plan ${id}`,
    overview: "",
    todoTotal: 0,
    todoCompleted: 0,
    createdAtMs: 0,
    updatedAtMs: 0,
  };
}

function record(id: string): PlanRecord {
  return {
    id,
    name: `plan ${id}`,
    overview: "",
    plan: "body",
    todos: [],
    createdAtMs: 0,
    updatedAtMs: 0,
    chatId: null,
    repoRoot: null,
  };
}

mock.module("../lib/plans", () => ({
  ...actualPlans,
  planList: async () => {
    listCalls += 1;
    if (listRejectsWith) throw listRejectsWith;
    return lists[Math.min(listCalls - 1, lists.length - 1)] ?? [];
  },
  planGet: async (planId: string) => {
    getCalls.push(planId);
    if (getRejectsFor === planId) throw "no such plan";
    return record(planId);
  },
  planDelete: async (planId: string) => {
    if (deleteRejectsWith) throw deleteRejectsWith;
    deleted.push(planId);
  },
}));

const { usePlans } = await import("../hooks/usePlans");

beforeEach(() => {
  lists = [];
  listCalls = 0;
  listRejectsWith = null;
  getCalls = [];
  getRejectsFor = null;
  deleted = [];
  deleteRejectsWith = null;
});

describe("usePlans", () => {
  test("loads the list and selects the first plan when none was asked for", async () => {
    lists.push([summary("a"), summary("b")]);
    const { result } = renderHook(() => usePlans(null));

    await waitFor(() => expect(result.current.summaries).toHaveLength(2));
    expect(result.current.selectedId).toBe("a");
    expect(result.current.loading).toBe(false);
    await waitFor(() => expect(result.current.detail?.id).toBe("a"));
  });

  test("an explicitly requested plan wins over the first one", async () => {
    lists.push([summary("a"), summary("b")]);
    const { result } = renderHook(() => usePlans("b"));

    await waitFor(() => expect(result.current.detail?.id).toBe("b"));
    expect(result.current.selectedId).toBe("b");
  });

  test("selecting a different plan loads its detail", async () => {
    lists.push([summary("a"), summary("b")]);
    const { result } = renderHook(() => usePlans(null));
    await waitFor(() => expect(result.current.detail?.id).toBe("a"));

    act(() => result.current.setSelectedId("b"));
    await waitFor(() => expect(result.current.detail?.id).toBe("b"));
    expect(getCalls).toEqual(["a", "b"]);
  });

  test("an empty list clears the selection instead of leaving a stale one", async () => {
    lists.push([]);
    const { result } = renderHook(() => usePlans(null));

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.selectedId).toBeNull();
    expect(result.current.detail).toBeNull();
    expect(getCalls).toHaveLength(0);
  });

  test("a failing list surfaces the message and empties the summaries", async () => {
    listRejectsWith = "store unavailable";
    const { result } = renderHook(() => usePlans(null));

    await waitFor(() => expect(result.current.error).toBe("store unavailable"));
    expect(result.current.summaries).toHaveLength(0);
    expect(result.current.loading).toBe(false);
  });

  test("a failing detail clears the pane and surfaces the message", async () => {
    lists.push([summary("a")]);
    getRejectsFor = "a";
    const { result } = renderHook(() => usePlans(null));

    await waitFor(() => expect(result.current.error).toBe("no such plan"));
    expect(result.current.detail).toBeNull();
  });

  test("deleting the selected plan falls back to the first survivor", async () => {
    lists.push([summary("a"), summary("b")], [summary("b")]);
    const { result } = renderHook(() => usePlans(null));
    await waitFor(() => expect(result.current.selectedId).toBe("a"));

    let ok: boolean | undefined;
    await act(async () => {
      ok = await result.current.deletePlan("a");
    });

    expect(ok).toBe(true);
    expect(deleted).toEqual(["a"]);
    // The selection can't survive — it no longer exists — so it lands on the
    // first remaining plan rather than on an empty pane.
    await waitFor(() => expect(result.current.selectedId).toBe("b"));
    expect(result.current.deleting).toBe(false);
  });

  test("deleting some other plan keeps the current selection", async () => {
    lists.push([summary("a"), summary("b")], [summary("a")]);
    const { result } = renderHook(() => usePlans(null));
    await waitFor(() => expect(result.current.selectedId).toBe("a"));

    await act(async () => {
      await result.current.deletePlan("b");
    });

    await waitFor(() => expect(result.current.summaries).toHaveLength(1));
    expect(result.current.selectedId).toBe("a");
  });

  test("a failing delete reports false so the caller keeps its confirmation open", async () => {
    lists.push([summary("a")]);
    deleteRejectsWith = "in use";
    const { result } = renderHook(() => usePlans(null));
    await waitFor(() => expect(result.current.selectedId).toBe("a"));

    let ok: boolean | undefined;
    await act(async () => {
      ok = await result.current.deletePlan("a");
    });

    expect(ok).toBe(false);
    expect(result.current.error).toBe("in use");
    expect(result.current.deleting).toBe(false);
  });
});
