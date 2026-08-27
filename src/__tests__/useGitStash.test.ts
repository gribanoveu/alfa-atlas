import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualGit from "../lib/git";
import type { GitStashEntry, GitStashRestoreOutcome } from "../lib/git";

let entries: GitStashEntry[] = [];
let listThrows: string | null = null;
let opThrows: string | null = null;
let restoreOutcome: GitStashRestoreOutcome = { conflicted: [] } as unknown as GitStashRestoreOutcome;
let calls: Array<[string, ...unknown[]]> = [];

mock.module("../lib/git", () => ({
  ...actualGit,
  gitStashList: async () => {
    if (listThrows) throw listThrows;
    return entries;
  },
  gitStashApply: async (...a: unknown[]) => {
    calls.push(["apply", ...a]);
    if (opThrows) throw opThrows;
    return restoreOutcome;
  },
  gitStashDrop: async (...a: unknown[]) => {
    calls.push(["drop", ...a]);
    if (opThrows) throw opThrows;
  },
}));

const { useGitStash } = await import("../hooks/useGitStash");

function entry(id: string, branch = "feature/x"): GitStashEntry {
  return { id, branch, message: id, time: 0 } as GitStashEntry;
}

beforeEach(() => {
  entries = [entry("s1")];
  listThrows = null;
  opThrows = null;
  restoreOutcome = { conflicted: [] } as unknown as GitStashRestoreOutcome;
  calls = [];
});

describe("useGitStash", () => {
  test("loads the shelf for an open repo", async () => {
    const { result } = renderHook(() => useGitStash("/repo"));
    await waitFor(() => expect(result.current.entries).toHaveLength(1));
    expect(result.current.error).toBeNull();
  });

  test("no repo means an empty shelf and no call", async () => {
    const { result } = renderHook(() => useGitStash(null));
    await waitFor(() => expect(result.current.entries).toEqual([]));
    expect(calls).toEqual([]);
  });

  test("an inactive shelf is not fetched", async () => {
    const { result } = renderHook(() => useGitStash("/repo", { active: false }));
    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current.entries).toEqual([]);
  });

  test("a failing list keeps whatever is on the shelf", async () => {
    const { result } = renderHook(() => useGitStash("/repo"));
    await waitFor(() => expect(result.current.entries).toHaveLength(1));

    listThrows = "stash ref unreadable";
    await act(async () => {
      await result.current.refresh();
    });

    expect(result.current.error).toBe("stash ref unreadable");
    expect(result.current.entries).toHaveLength(1);
  });

  test("restoring returns the outcome so conflicts can be surfaced", async () => {
    // A restore can land in conflict; the caller needs to know which files
    // rather than just whether it "worked".
    restoreOutcome = { conflicted: ["a.adoc"] } as unknown as GitStashRestoreOutcome;
    const { result } = renderHook(() => useGitStash("/repo"));
    await waitFor(() => expect(result.current.entries).toHaveLength(1));
    entries = [];

    let outcome: GitStashRestoreOutcome | null = null;
    await act(async () => {
      outcome = await result.current.restore("s1");
    });

    expect(calls[0]).toEqual(["apply", "/repo", "s1"]);
    expect(outcome).toMatchObject({ conflicted: ["a.adoc"] });
    expect(result.current.entries).toEqual([]);
    expect(result.current.busy).toBe(false);
  });

  test("a failed restore returns null rather than a misleading outcome", async () => {
    opThrows = "stash entry missing";
    const { result } = renderHook(() => useGitStash("/repo"));
    await waitFor(() => expect(result.current.entries).toHaveLength(1));

    let outcome: GitStashRestoreOutcome | null = restoreOutcome;
    await act(async () => {
      outcome = await result.current.restore("s1");
    });

    expect(outcome).toBeNull();
    expect(result.current.error).toBe("stash entry missing");
  });

  test("discarding drops the entry and refreshes", async () => {
    const { result } = renderHook(() => useGitStash("/repo"));
    await waitFor(() => expect(result.current.entries).toHaveLength(1));
    entries = [];

    let ok = false;
    await act(async () => {
      ok = await result.current.discard("s1");
    });

    expect(ok).toBe(true);
    expect(calls[0]).toEqual(["drop", "/repo", "s1"]);
    expect(result.current.entries).toEqual([]);
  });

  test("a failed discard reports false", async () => {
    opThrows = "cannot drop";
    const { result } = renderHook(() => useGitStash("/repo"));
    await waitFor(() => expect(result.current.entries).toHaveLength(1));

    let ok = true;
    await act(async () => {
      ok = await result.current.discard("s1");
    });

    expect(ok).toBe(false);
    expect(result.current.error).toBe("cannot drop");
  });

  test("operations without a repo do nothing", async () => {
    const { result } = renderHook(() => useGitStash(null));
    await act(async () => {
      expect(await result.current.restore("s1")).toBeNull();
      expect(await result.current.discard("s1")).toBe(false);
    });
    expect(calls).toEqual([]);
  });
});
