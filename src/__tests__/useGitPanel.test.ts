import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualGit from "../lib/git";
import type { GitCommitSummary, GitStatusSnapshot } from "../lib/git";

const EMPTY: GitStatusSnapshot = {
  staged: [],
  unstaged: [],
  conflicted: [],
  branch: null,
  hasCommits: false,
  hasUpstream: false,
  ahead: 0,
  mergeInProgress: false,
};

let status: GitStatusSnapshot;
let commits: GitCommitSummary[];
let statusThrows: string | null = null;
let logThrows: string | null = null;
let opThrows: string | null = null;
let calls: Array<[string, ...unknown[]]> = [];
let commitHash = "abc1234";
/** Status the *second* read returns — how a conflicted pull surfaces. */
let statusAfterPull: GitStatusSnapshot | null = null;

mock.module("../lib/git", () => ({
  ...actualGit,
  gitStatus: async () => {
    if (statusThrows) throw statusThrows;
    if (statusAfterPull) {
      const next = statusAfterPull;
      statusAfterPull = null;
      return next;
    }
    return status;
  },
  gitLog: async () => {
    if (logThrows) throw logThrows;
    return commits;
  },
  gitStage: async (...a: unknown[]) => {
    calls.push(["stage", ...a]);
    if (opThrows) throw opThrows;
  },
  gitUnstage: async (...a: unknown[]) => {
    calls.push(["unstage", ...a]);
    if (opThrows) throw opThrows;
  },
  gitCommit: async (...a: unknown[]) => {
    calls.push(["commit", ...a]);
    if (opThrows) throw opThrows;
    return commitHash;
  },
  gitPull: async (...a: unknown[]) => {
    calls.push(["pull", ...a]);
    if (opThrows) throw opThrows;
  },
  gitPush: async (...a: unknown[]) => {
    calls.push(["push", ...a]);
    if (opThrows) throw opThrows;
  },
}));
mock.module("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

const { useGitPanel, formatDocCommitMessage } = await import("../hooks/useGitPanel");

function file(path: string) {
  return { path, status: "M" };
}

function render(repoRoot: string | null = "/repo", active = true, onBranchChange?: (b: string | null) => void) {
  return renderHook(() => useGitPanel(repoRoot, { active, onBranchChange }));
}

beforeEach(() => {
  status = { ...EMPTY, branch: "main", hasCommits: true };
  commits = [];
  statusThrows = null;
  logThrows = null;
  opThrows = null;
  statusAfterPull = null;
  calls = [];
  commitHash = "abc1234";
});

describe("formatDocCommitMessage", () => {
  test("wraps the description in the doc() convention", () => {
    expect(formatDocCommitMessage("JIRA-1", "правки")).toBe("doc(JIRA-1): правки");
  });

  test("an empty Jira key still produces a valid message", () => {
    expect(formatDocCommitMessage("", "правки")).toBe("doc(): правки");
  });

  test("no description means no message at all", () => {
    expect(formatDocCommitMessage("JIRA-1", "   ")).toBeNull();
  });

  test("surrounding whitespace is trimmed from both parts", () => {
    expect(formatDocCommitMessage("  JIRA-1  ", "  правки  ")).toBe("doc(JIRA-1): правки");
  });
});

describe("useGitPanel — loading", () => {
  test("loads status and log for an open repo", async () => {
    status = { ...status, unstaged: [file("a.adoc")] };
    commits = [{ hash: "a1", message: "m", author: "u", time: 0 }];
    const { result } = render();

    await waitFor(() => expect(result.current.status.unstaged).toHaveLength(1));
    expect(result.current.commits).toHaveLength(1);
    expect(result.current.error).toBeNull();
  });

  test("the branch is reported outward as it is read", async () => {
    const seen: Array<string | null> = [];
    render("/repo", true, (b) => seen.push(b));
    await waitFor(() => expect(seen).toContain("main"));
  });

  test("no repo means an empty snapshot and a null branch", async () => {
    const seen: Array<string | null> = [];
    const { result } = render(null, true, (b) => seen.push(b));
    await waitFor(() => expect(result.current.status.branch).toBeNull());
    expect(result.current.commits).toEqual([]);
  });

  test("a collapsed panel does not poll git", async () => {
    const { result } = render("/repo", false);
    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current.status.branch).toBeNull();
  });

  test("a failing log still leaves the status usable", async () => {
    // The two reads are independent; losing the history should not blank
    // the staging area.
    status = { ...status, unstaged: [file("a.adoc")] };
    logThrows = "bad object";
    const { result } = render();

    await waitFor(() => expect(result.current.error).toBe("bad object"));
    expect(result.current.status.unstaged).toHaveLength(1);
  });

  test("a failing status reports why", async () => {
    statusThrows = "not a git repository";
    const { result } = render();
    await waitFor(() => expect(result.current.error).toBe("not a git repository"));
  });
});

describe("useGitPanel — staging and committing", () => {
  test("staging and unstaging pass the paths through", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.status.branch).toBe("main"));

    await act(async () => {
      await result.current.stage(["a.adoc"]);
    });
    await act(async () => {
      await result.current.unstage(["a.adoc"]);
    });

    expect(calls).toEqual([
      ["stage", "/repo", ["a.adoc"]],
      ["unstage", "/repo", ["a.adoc"]],
    ]);
  });

  test("staging nothing is not a git call", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.status.branch).toBe("main"));

    await act(async () => {
      await result.current.stage([]);
    });
    expect(calls).toEqual([]);
  });

  test("commit is blocked until something is staged and described", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.status.branch).toBe("main"));
    expect(result.current.canCommit).toBe(false);

    act(() => result.current.setDescription("правки"));
    expect(result.current.canCommit).toBe(false);

    status = { ...status, staged: [file("a.adoc")] };
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.canCommit).toBe(true);
  });

  test("committing sends the formatted message and clears the form", async () => {
    status = { ...status, staged: [file("a.adoc")] };
    const { result } = render();
    await waitFor(() => expect(result.current.status.staged).toHaveLength(1));

    act(() => {
      result.current.setJiraKey("JIRA-7");
      result.current.setDescription("правки");
    });

    let hash: string | null = null;
    await act(async () => {
      hash = await result.current.commit();
    });

    expect(hash).toBe("abc1234");
    expect(calls[0]).toEqual(["commit", "/repo", "doc(JIRA-7): правки"]);
    expect(result.current.jiraKey).toBe("");
    expect(result.current.description).toBe("");
  });

  test("committing with nothing staged does not reach git", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.status.branch).toBe("main"));
    act(() => result.current.setDescription("правки"));

    await act(async () => {
      expect(await result.current.commit()).toBeNull();
    });
    expect(calls).toEqual([]);
  });

  test("a failed commit returns null and keeps the message for a retry", async () => {
    status = { ...status, staged: [file("a.adoc")] };
    opThrows = "user.name is not configured";
    const { result } = render();
    await waitFor(() => expect(result.current.status.staged).toHaveLength(1));
    act(() => result.current.setDescription("правки"));

    await act(async () => {
      expect(await result.current.commit()).toBeNull();
    });

    expect(result.current.error).toBe("user.name is not configured");
    expect(result.current.description).toBe("правки");
  });
});

describe("useGitPanel — pull", () => {
  test("a clean pull reports ok", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.status.branch).toBe("main"));

    await act(async () => {
      expect(await result.current.pull("merge")).toEqual({ status: "ok" });
    });
  });

  test("a conflicted pull is not an error — it is a state", async () => {
    // The repo is left mid-merge, and the panel shows the conflicts rather
    // than an alert about a failure.
    const { result } = render();
    await waitFor(() => expect(result.current.status.branch).toBe("main"));
    opThrows = "automatic merge failed";
    statusAfterPull = { ...status, conflicted: [file("a.adoc")], mergeInProgress: true };

    await act(async () => {
      expect(await result.current.pull("merge")).toEqual({ status: "conflict" });
    });

    expect(result.current.error).toBeNull();
    expect(result.current.status.conflicted).toHaveLength(1);
  });

  test("a pull that failed for any other reason reports the message", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.status.branch).toBe("main"));
    opThrows = "could not read from remote";

    await act(async () => {
      const outcome = await result.current.pull("merge");
      expect(outcome).toMatchObject({ status: "error", message: "could not read from remote" });
    });
    expect(result.current.error).toBe("could not read from remote");
  });

  test("pushing without a repo reports rather than throwing", async () => {
    const { result } = render(null);
    await act(async () => {
      expect(await result.current.push()).toBe("Нет открытого репозитория");
    });
  });
});
