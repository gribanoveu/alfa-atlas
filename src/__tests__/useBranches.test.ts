import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualGit from "../lib/git";
import type { CheckoutOutcome, GitBranchInfo } from "../lib/git";

let listed: GitBranchInfo[] = [];
let listThrows: string | null = null;
let opThrows: string | null = null;
let checkoutOutcome: CheckoutOutcome = { shelved: null } as CheckoutOutcome;
let calls: Array<[string, ...unknown[]]> = [];

mock.module("../lib/git", () => ({
  ...actualGit,
  gitListBranches: async () => {
    if (listThrows) throw listThrows;
    return listed;
  },
  gitCreateBranch: async (...a: unknown[]) => {
    calls.push(["create", ...a]);
    if (opThrows) throw opThrows;
  },
  gitDeleteBranch: async (...a: unknown[]) => {
    calls.push(["delete", ...a]);
    if (opThrows) throw opThrows;
  },
  gitFetchBranches: async (...a: unknown[]) => {
    calls.push(["fetch", ...a]);
    if (opThrows) throw opThrows;
  },
  gitCheckoutBranch: async (...a: unknown[]) => {
    calls.push(["checkout", ...a]);
    if (opThrows) throw opThrows;
    return checkoutOutcome;
  },
  gitCheckoutRemoteBranch: async (...a: unknown[]) => {
    calls.push(["checkoutRemote", ...a]);
    if (opThrows) throw opThrows;
    return checkoutOutcome;
  },
}));

const { useBranches } = await import("../hooks/useBranches");

function branch(name: string, isCurrent = false): GitBranchInfo {
  return { name, isCurrent, isRemote: false, behind: 0, ahead: 0 } as GitBranchInfo;
}

beforeEach(() => {
  listed = [branch("main", true), branch("feature/x")];
  listThrows = null;
  opThrows = null;
  checkoutOutcome = { shelved: null } as CheckoutOutcome;
  calls = [];
});

describe("useBranches", () => {
  test("loads the branch list for an open repo", async () => {
    const { result } = renderHook(() => useBranches("/repo"));
    await waitFor(() => expect(result.current.branches).toHaveLength(2));
    expect(result.current.error).toBeNull();
  });

  test("no repo means an empty list and no call", async () => {
    const { result } = renderHook(() => useBranches(null));
    await waitFor(() => expect(result.current.branches).toEqual([]));
    expect(calls).toEqual([]);
  });

  test("inactive means the list is not fetched", async () => {
    // The branches panel is collapsed — no reason to hit git.
    const { result } = renderHook(() => useBranches("/repo", { active: false }));
    await Promise.resolve();
    expect(result.current.branches).toEqual([]);
  });

  test("becoming active fetches", async () => {
    const { result, rerender } = renderHook(
      ({ active }) => useBranches("/repo", { active }),
      { initialProps: { active: false } },
    );
    expect(result.current.branches).toEqual([]);

    rerender({ active: true });
    await waitFor(() => expect(result.current.branches).toHaveLength(2));
  });

  test("a failing list reports why and keeps what it had", async () => {
    const { result } = renderHook(() => useBranches("/repo"));
    await waitFor(() => expect(result.current.branches).toHaveLength(2));

    listThrows = "not a git repository";
    await act(async () => {
      await result.current.refresh();
    });

    expect(result.current.error).toBe("not a git repository");
    expect(result.current.branches).toHaveLength(2);
  });

  test("creating a branch refreshes the list and reports success", async () => {
    const { result } = renderHook(() => useBranches("/repo"));
    await waitFor(() => expect(result.current.branches).toHaveLength(2));
    listed = [...listed, branch("feature/y")];

    let ok = false;
    await act(async () => {
      ok = await result.current.createBranch("feature/y");
    });

    expect(ok).toBe(true);
    expect(calls[0]).toEqual(["create", "/repo", "feature/y", false]);
    expect(result.current.branches).toHaveLength(3);
    expect(result.current.busy).toBe(false);
  });

  test("a failed operation reports false and the reason", async () => {
    const { result } = renderHook(() => useBranches("/repo"));
    await waitFor(() => expect(result.current.branches).toHaveLength(2));
    opThrows = "branch already exists";

    let ok = true;
    await act(async () => {
      ok = await result.current.createBranch("feature/x");
    });

    expect(ok).toBe(false);
    expect(result.current.error).toBe("branch already exists");
    expect(result.current.busy).toBe(false);
  });

  test("deleting and fetching go through the same path", async () => {
    const { result } = renderHook(() => useBranches("/repo"));
    await waitFor(() => expect(result.current.branches).toHaveLength(2));

    await act(async () => {
      await result.current.deleteBranch("feature/x");
      await result.current.fetchBranches();
    });

    expect(calls.map((c) => c[0])).toEqual(["delete", "fetch"]);
  });

  test("checkout returns the outcome, not just a boolean", async () => {
    // The caller needs to know whether changes were shelved, to drive the
    // post-checkout toast and restore flow.
    checkoutOutcome = { shelved: { branch: "main", id: "s1" } } as CheckoutOutcome;
    const { result } = renderHook(() => useBranches("/repo"));
    await waitFor(() => expect(result.current.branches).toHaveLength(2));

    let outcome: CheckoutOutcome | null = null;
    await act(async () => {
      outcome = await result.current.checkoutBranch("feature/x");
    });

    expect(outcome).toMatchObject({ shelved: { branch: "main" } });
  });

  test("a failed checkout returns null rather than a misleading outcome", async () => {
    opThrows = "local changes would be overwritten";
    const { result } = renderHook(() => useBranches("/repo"));
    await waitFor(() => expect(result.current.branches).toHaveLength(2));

    let outcome: CheckoutOutcome | null = { shelved: null } as CheckoutOutcome;
    await act(async () => {
      outcome = await result.current.checkoutBranch("feature/x");
    });

    expect(outcome).toBeNull();
    expect(result.current.error).toBe("local changes would be overwritten");
  });

  test("a remote checkout passes the discard flag through", async () => {
    const { result } = renderHook(() => useBranches("/repo"));
    await waitFor(() => expect(result.current.branches).toHaveLength(2));

    await act(async () => {
      await result.current.checkoutRemoteBranch("origin/feature/z", true);
    });

    expect(calls[0]).toEqual(["checkoutRemote", "/repo", "origin/feature/z", true]);
  });

  test("operations without a repo do nothing", async () => {
    const { result } = renderHook(() => useBranches(null));
    await Promise.resolve();

    await act(async () => {
      expect(await result.current.createBranch("x")).toBe(false);
      expect(await result.current.checkoutBranch("x")).toBeNull();
    });
    expect(calls).toEqual([]);
  });
});
