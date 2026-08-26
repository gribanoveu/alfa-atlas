import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualProject from "../lib/project";
import type { ProbeResult } from "../lib/project";

type Probe = ProbeResult;

let current: { root: string; docsRoot: string } | null = null;
let savedRoot: string | null = null;
let probes: Record<string, Probe> = {};
let branch: string | null = "main";
let branchThrows = false;
let getProjectThrows: string | null = null;
let cleared = false;
let openedWith: Array<[string, string]> = [];

mock.module("../lib/project", () => ({
  ...actualProject,
  getProject: async () => {
    if (getProjectThrows) throw getProjectThrows;
    return current;
  },
  getSavedRepoRoot: async () => savedRoot,
  probeOpenPath: async (p: string) => probes[p] ?? { root: p, docsRoot: null, needsConfirm: true },
  openCachedProject: async (root: string) => ({ root, docsRoot: probes[root]?.docsRoot ?? root }),
  openProject: async (root: string, docs: string) => {
    openedWith.push([root, docs]);
    return { root, docsRoot: docs };
  },
  clearProject: async () => {
    cleared = true;
  },
  getGitBranch: async () => {
    if (branchThrows) throw new Error("not a repo");
    return branch;
  },
}));
let pickResult: string | null = null;
mock.module("@tauri-apps/plugin-dialog", () => ({ open: async () => pickResult }));

const { useProject } = await import("../hooks/useProject");

function probe(root: string, docsRoot: string | null, needsConfirm: boolean): Probe {
  return { root, docsRoot, needsConfirm } as Probe;
}

beforeEach(() => {
  current = null;
  savedRoot = null;
  probes = {};
  branch = "main";
  branchThrows = false;
  getProjectThrows = null;
  cleared = false;
  openedWith = [];
  pickResult = null;
});

describe("useProject — startup", () => {
  test("an already-open project is adopted as is", async () => {
    current = { root: "/repo", docsRoot: "/repo/docs" };
    const { result } = renderHook(() => useProject());

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.repoRoot).toBe("/repo");
    expect(result.current.docsRoot).toBe("/repo/docs");
    expect(result.current.branchName).toBe("main");
  });

  test("with nothing open, a remembered repo is reopened silently", async () => {
    savedRoot = "/repo";
    probes["/repo"] = probe("/repo", "/repo/docs", false);
    const { result } = renderHook(() => useProject());

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.repoRoot).toBe("/repo");
    expect(result.current.pendingOpen).toBeNull();
  });

  test("a remembered repo whose docs root is unclear asks first", async () => {
    // Reopening into the wrong folder would be worse than one prompt.
    savedRoot = "/repo";
    probes["/repo"] = probe("/repo", null, true);
    const { result } = renderHook(() => useProject());

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.repoRoot).toBeNull();
    expect(result.current.pendingOpen).toMatchObject({ root: "/repo" });
  });

  test("nothing open and nothing remembered leaves the welcome screen", async () => {
    const { result } = renderHook(() => useProject());
    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.repoRoot).toBeNull();
    expect(result.current.error).toBeNull();
  });

  test("a failing restore still finishes loading, with the reason", async () => {
    // `ready` must flip regardless, or the app would sit on a splash forever.
    getProjectThrows = "settings unreadable";
    const { result } = renderHook(() => useProject());

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.error).toBe("settings unreadable");
    expect(result.current.repoRoot).toBeNull();
  });

  test("a repo with no git branch still opens", async () => {
    current = { root: "/repo", docsRoot: "/repo/docs" };
    branchThrows = true;
    const { result } = renderHook(() => useProject());

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.repoRoot).toBe("/repo");
    expect(result.current.branchName).toBeNull();
  });
});

describe("useProject — opening", () => {
  test("an unambiguous path opens without asking", async () => {
    probes["/other"] = probe("/other", "/other/docs", false);
    const { result } = renderHook(() => useProject());
    await waitFor(() => expect(result.current.ready).toBe(true));

    await act(async () => {
      await result.current.beginOpenPath("/other");
    });

    expect(result.current.repoRoot).toBe("/other");
    expect(result.current.pendingOpen).toBeNull();
  });

  test("an ambiguous path raises a confirmation instead of opening", async () => {
    probes["/other"] = probe("/other", null, true);
    const { result } = renderHook(() => useProject());
    await waitFor(() => expect(result.current.ready).toBe(true));

    await act(async () => {
      await result.current.beginOpenPath("/other");
    });

    expect(result.current.repoRoot).toBeNull();
    expect(result.current.pendingOpen).toMatchObject({ root: "/other" });
  });

  test("confirming opens with the docs root the user chose", async () => {
    probes["/other"] = probe("/other", null, true);
    const { result } = renderHook(() => useProject());
    await waitFor(() => expect(result.current.ready).toBe(true));
    await act(async () => {
      await result.current.beginOpenPath("/other");
    });

    await act(async () => {
      await result.current.confirmPendingOpen("/other/documentation");
    });

    expect(openedWith).toEqual([["/other", "/other/documentation"]]);
    expect(result.current.docsRoot).toBe("/other/documentation");
    expect(result.current.pendingOpen).toBeNull();
  });

  test("confirming with nothing pending is a no-op", async () => {
    const { result } = renderHook(() => useProject());
    await waitFor(() => expect(result.current.ready).toBe(true));

    await act(async () => {
      const opened = await result.current.confirmPendingOpen("/x/docs");
      expect(opened).toBeNull();
    });
    expect(openedWith).toEqual([]);
  });

  test("cancelling drops the pending open without touching the project", async () => {
    current = { root: "/repo", docsRoot: "/repo/docs" };
    probes["/other"] = probe("/other", null, true);
    const { result } = renderHook(() => useProject());
    await waitFor(() => expect(result.current.ready).toBe(true));
    await act(async () => {
      await result.current.beginOpenPath("/other");
    });

    act(() => result.current.cancelPendingOpen());

    expect(result.current.pendingOpen).toBeNull();
    expect(result.current.repoRoot).toBe("/repo");
  });

  test("a cancelled folder dialog opens nothing", async () => {
    const { result } = renderHook(() => useProject());
    await waitFor(() => expect(result.current.ready).toBe(true));

    await act(async () => {
      const opened = await result.current.openFolderDialog();
      expect(opened).toBeNull();
    });
    expect(result.current.repoRoot).toBeNull();
  });
});

describe("useProject — state", () => {
  test("the project name is the last path segment", async () => {
    current = { root: "/home/u/corp-wlbuh-ausn-api", docsRoot: "/home/u/corp-wlbuh-ausn-api/docs" };
    const { result } = renderHook(() => useProject());
    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.projectName).toBe("corp-wlbuh-ausn-api");
  });

  test("closing clears everything and tells the backend", async () => {
    current = { root: "/repo", docsRoot: "/repo/docs" };
    const { result } = renderHook(() => useProject());
    await waitFor(() => expect(result.current.repoRoot).not.toBeNull());

    await act(async () => {
      await result.current.closeProject();
    });

    expect(cleared).toBe(true);
    expect(result.current.repoRoot).toBeNull();
    expect(result.current.docsRoot).toBeNull();
    expect(result.current.branchName).toBeNull();
  });

  test("refreshing the branch with no project clears it", async () => {
    const { result } = renderHook(() => useProject());
    await waitFor(() => expect(result.current.ready).toBe(true));

    await act(async () => {
      await result.current.refreshBranch();
    });
    expect(result.current.branchName).toBeNull();
  });

  test("a checkout can push the new branch in without a round trip", async () => {
    current = { root: "/repo", docsRoot: "/repo/docs" };
    const { result } = renderHook(() => useProject());
    await waitFor(() => expect(result.current.branchName).toBe("main"));

    act(() => result.current.setBranchFromGit("feature/x"));
    expect(result.current.branchName).toBe("feature/x");
  });
});
