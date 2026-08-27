import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook } from "@testing-library/react";
import * as actualGit from "../lib/git";

let syncStatus: { behind: number; ahead: number } = { behind: 0, ahead: 0 };
let syncStatusThrows: string | null = null;

mock.module("../lib/git", () => ({
  ...actualGit,
  gitSyncStatus: async () => {
    if (syncStatusThrows) throw syncStatusThrows;
    return syncStatus;
  },
  gitUnpushedCommits: async () => [
    { hash: "abc1234", message: "local change", author: "Test", time: 1 },
  ],
  gitIncomingCommits: async () => [
    { hash: "def5678", message: "remote change", author: "Test", time: 2 },
  ],
  gitHeadOid: async () => "abc123",
  gitCommitFiles: async () => [],
  gitCommitFileDiff: async () => ({}),
  gitCreateBranchAtOid: async () => {},
  gitResetToOid: async () => {},
  gitRestoreDiscardBackup: async () => {},
  gitUndoCommit: async () => {},
  gitDropUnpushedFrom: async () => {},
  gitDropAllUnpushed: async () => {},
  gitMoveUnpushedToNewBranch: async () => {},
  gitMoveUnpushedToBranch: async () => {},
  deriveSyncPillState: () => "synced",
}));

const { useGitWorkflow } = await import("../hooks/useGitWorkflow");

let pushResult: string | null = null;
let pullResult: { status: string; message?: string } = { status: "ok" };
let recorded: unknown[] = [];
let successes: string[] = [];

function makeDeps() {
  return {
    hasProject: true,
    project: { repoRoot: "/repo", docsRoot: "/repo/docs", refreshBranch: mock(async () => {}) },
    git: {
      status: {
        conflicted: [],
        mergeInProgress: false,
        staged: [],
        unstaged: [],
        hasUpstream: false,
        ahead: 0,
      },
      unpushedCommits: [],
      push: mock(async () => pushResult),
      pull: mock(async () => pullResult),
      refresh: mock(async () => {}),
      resetToRemote: mock(async () => null),
    },
    branches: { branches: [], refresh: mock(async () => {}), deleteBranch: mock(async () => true), error: null },
    stash: { refresh: mock(async () => {}), discard: mock(async () => {}), error: null },
    actionLog: { record: mock((entry: unknown) => recorded.push(entry)) },
    editor: { reloadAllOpenTabs: mock(async () => {}), saveAllDirtyTabs: mock(async () => true) },
    tree: { refresh: mock(async () => {}) },
    layout: { activeTool: null, setRightTool: mock(() => {}), setBottomToolId: mock(() => {}) },
    showSuccess: mock((m: string) => successes.push(m)),
  };
}

function render(overrides: Partial<ReturnType<typeof makeDeps>> = {}) {
  const deps = { ...makeDeps(), ...overrides };
  return { deps, ...renderHook(() => useGitWorkflow(deps as never)) };
}

beforeEach(() => {
  syncStatus = { behind: 0, ahead: 0 };
  syncStatusThrows = null;
  pushResult = null;
  pullResult = { status: "ok" };
  recorded = [];
  successes = [];
});

describe("useGitWorkflow — modals", () => {
  test("every dialog starts closed", () => {
    const { result } = render();
    expect(result.current.pullModalOpen).toBe(false);
    expect(result.current.pushConfirmOpen).toBe(false);
    expect(result.current.resetRemoteConfirmOpen).toBe(false);
    expect(result.current.gitAlert).toBeNull();
    expect(result.current.deleteBranchTarget).toBeNull();
    expect(result.current.dropUnpushedTarget).toBeNull();
    expect(result.current.dropAllUnpushedOpen).toBe(false);
    expect(result.current.moveUnpushedOpen).toBe(false);
  });

  test("the pull dialog does not open without a project", () => {
    const { result } = render({ hasProject: false });
    act(() => result.current.openPullModal());
    expect(result.current.pullModalOpen).toBe(false);
  });

  test("the pull dialog opens with a project", () => {
    const { result } = render();
    act(() => result.current.openPullModal());
    expect(result.current.pullModalOpen).toBe(true);
  });

  test("the push dialog opens with a project", () => {
    const { result } = render();
    act(() => result.current.openPushModal());
    expect(result.current.pushConfirmOpen).toBe(true);
  });
});

describe("useGitWorkflow — push", () => {
  test("a successful push toasts and records an entry", async () => {
    const { result } = render();
    await act(async () => {
      await result.current.runPush();
    });

    expect(successes).toEqual(["Изменения отправлены на сервер"]);
    expect(recorded).toHaveLength(1);
    expect(recorded[0]).toMatchObject({ kind: "push", undoable: false });
    expect(result.current.gitAlert).toBeNull();
  });

  test("being behind the remote blocks the push before it starts", async () => {
    syncStatus = { behind: 3, ahead: 1 };
    const { deps, result } = render();

    await act(async () => {
      await result.current.runPush();
    });

    expect(deps.git.push).not.toHaveBeenCalled();
    expect(result.current.gitAlert?.variant).toBe("info");
    // Russian pluralisation: 3 -> "новых коммита".
    expect(result.current.gitAlert?.message).toContain("3 новых коммита");
    expect(recorded).toHaveLength(0);
  });

  test("one commit behind uses the singular form", async () => {
    syncStatus = { behind: 1, ahead: 0 };
    const { result } = render();
    await act(async () => {
      await result.current.runPush();
    });
    expect(result.current.gitAlert?.message).toContain("1 новый коммит");
  });

  test("eleven behind is not treated as one", async () => {
    // 11 % 10 === 1, so a naive rule would say "новый коммит".
    syncStatus = { behind: 11, ahead: 0 };
    const { result } = render();
    await act(async () => {
      await result.current.runPush();
    });
    expect(result.current.gitAlert?.message).toContain("11 новых коммитов");
  });

  test("a rejected push surfaces the reason and records nothing", async () => {
    pushResult = "authentication failed";
    const { result } = render();

    await act(async () => {
      await result.current.runPush();
    });

    expect(result.current.gitAlert?.message).toBe("authentication failed");
    expect(successes).toEqual([]);
    // Nothing happened, so there is nothing to offer an undo for.
    expect(recorded).toHaveLength(0);
  });

  test("a thrown failure becomes an alert instead of an unhandled rejection", async () => {
    syncStatusThrows = "network unreachable";
    const { result } = render();

    await act(async () => {
      await result.current.runPush();
    });

    expect(result.current.gitAlert?.message).toBe("network unreachable");
  });

  test("push does nothing at all without a project", async () => {
    const { deps, result } = render({ hasProject: false });
    await act(async () => {
      await result.current.runPush();
    });
    expect(deps.git.push).not.toHaveBeenCalled();
    expect(result.current.gitAlert).toBeNull();
  });

  test("confirming keeps the dialog open until push completes", async () => {
    const { deps, result } = render();
    act(() => result.current.setPushConfirmOpen(true));

    let resolvePush: () => void;
    deps.git.push = mock(
      () =>
        new Promise<string | null>((resolve) => {
          resolvePush = () => resolve(null);
        }),
    );

    let confirmPromise!: Promise<void>;
    await act(async () => {
      confirmPromise = result.current.onPushConfirm();
      await Promise.resolve();
    });

    expect(result.current.pushConfirmOpen).toBe(true);
    expect(deps.git.push).toHaveBeenCalled();

    await act(async () => {
      resolvePush!();
      await confirmPromise;
    });

    expect(result.current.pushConfirmOpen).toBe(false);
  });
});

describe("useGitWorkflow — commit lists", () => {
  test("opening the push dialog loads unpushed commits", async () => {
    const { result } = render();
    act(() => result.current.openPushModal());

    await act(async () => {
      await Promise.resolve();
    });

    expect(result.current.pushCommits).toEqual([
      { hash: "abc1234", message: "local change", author: "Test", time: 1 },
    ]);
    expect(result.current.pushCommitsLoading).toBe(false);
  });

  test("opening the pull dialog loads incoming commits", async () => {
    const { result } = render();
    act(() => result.current.openPullModal());

    await act(async () => {
      await Promise.resolve();
    });

    expect(result.current.pullCommits).toEqual([
      { hash: "def5678", message: "remote change", author: "Test", time: 2 },
    ]);
    expect(result.current.pullCommitsLoading).toBe(false);
  });
});

describe("useGitWorkflow — pull", () => {
  test("a successful pull closes the dialog and toasts", async () => {
    const { result } = render();
    act(() => result.current.openPullModal());

    await act(async () => {
      await result.current.onPullConfirm("rebase" as never);
    });

    expect(result.current.pullModalOpen).toBe(false);
    expect(successes).toEqual(["Проект обновлён"]);
  });

  test("a failed pull closes the dialog and alerts", async () => {
    pullResult = { status: "error", message: "diverged" };
    const { result } = render();
    act(() => result.current.openPullModal());

    await act(async () => {
      await result.current.onPullConfirm("rebase" as never);
    });

    expect(result.current.pullModalOpen).toBe(false);
    expect(result.current.gitAlert?.message).toBe("diverged");
    expect(successes).toEqual([]);
  });

  test("a conflicting pull raises no alert — the git panel shows it", async () => {
    pullResult = { status: "conflict" };
    const { result } = render();

    await act(async () => {
      await result.current.onPullConfirm("rebase" as never);
    });

    expect(result.current.gitAlert).toBeNull();
    expect(successes).toEqual([]);
  });
});

describe("useGitWorkflow — create branch", () => {
  test("creates a branch even with uncommitted changes", async () => {
    const createBranch = mock(async () => true);
    const setBranchFromGit = mock(() => {});
    const { deps, result } = render({
      git: {
        ...makeDeps().git,
        status: {
          conflicted: [],
          mergeInProgress: false,
          staged: [{ path: "a.txt", status: "M" }],
          unstaged: [],
          hasUpstream: true,
          ahead: 0,
        },
        unpushedCommits: [],
      },
      branches: {
        ...makeDeps().branches,
        createBranch,
      },
      project: {
        ...makeDeps().project,
        setBranchFromGit,
      },
    });

    await act(async () => {
      await result.current.handleCreateBranch("feature/x");
    });

    expect(createBranch).toHaveBeenCalledWith("feature/x", false);
    expect(setBranchFromGit).toHaveBeenCalledWith("feature/x");
  });
});

describe("useGitWorkflow — branch deletion", () => {
  test("confirming deletes, closes, and records an undoable entry", async () => {
    const { deps, result } = render();
    act(() =>
      result.current.setDeleteBranchTarget({ name: "feature/x", tipOid: "deadbeef" } as never),
    );

    await act(async () => {
      await result.current.onDeleteBranchConfirm();
    });

    expect(deps.branches.deleteBranch).toHaveBeenCalledWith("feature/x");
    expect(result.current.deleteBranchTarget).toBeNull();
    expect(recorded[0]).toMatchObject({ kind: "deleteBranch", undoable: true });
  });

  test("a branch with no tip records nothing — there is no commit to restore", async () => {
    const { result } = render();
    act(() => result.current.setDeleteBranchTarget({ name: "feature/x", tipOid: null } as never));

    await act(async () => {
      await result.current.onDeleteBranchConfirm();
    });

    expect(recorded).toHaveLength(0);
  });

  test("a refused deletion alerts and records nothing", async () => {
    const deps = makeDeps();
    deps.branches.deleteBranch = mock(async () => false);
    deps.branches.error = "branch is checked out" as never;
    const { result } = renderHook(() => useGitWorkflow(deps as never));
    act(() => result.current.setDeleteBranchTarget({ name: "x", tipOid: "a" } as never));

    await act(async () => {
      await result.current.onDeleteBranchConfirm();
    });

    expect(result.current.gitAlert?.message).toBe("branch is checked out");
    expect(recorded).toHaveLength(0);
  });
});
