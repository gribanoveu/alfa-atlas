import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualGit from "../lib/git";
import * as actualProject from "../lib/project";
import * as actualPrefs from "../lib/prefs";

let cloneResult: unknown = { root: "/repo" };
let cloneError: string | null = null;
let pathIsNonEmpty = false;
let cancelledIds: string[] = [];
let resolveClone: ((value: unknown) => void) | null = null;
let lastCloneDir: string | null = null;
let savedPrefs: unknown[] = [];
let pickResult: string | null = null;

mock.module("../lib/git", () => ({
  ...actualGit,
  gitClone: async () => {
    if (cloneError) throw cloneError;
    if (resolveClone) {
      // Lets a test hold a clone open and cancel it mid-flight.
      return await new Promise((resolve) => {
        resolveClone = resolve;
      });
    }
    return cloneResult;
  },
  gitCloneCancel: async (cloneId: string) => {
    cancelledIds.push(cloneId);
  },
}));
mock.module("../lib/project", () => ({
  ...actualProject,
  checkPathExists: async () => ({
    exists: pathIsNonEmpty,
    isDir: pathIsNonEmpty,
    isNonEmpty: pathIsNonEmpty,
  }),
}));
mock.module("../lib/prefs", () => ({
  ...actualPrefs,
  getGeneralPrefs: async () => ({ lastCloneDir }),
  setGeneralPrefs: async (p: unknown) => {
    savedPrefs.push(p);
  },
}));
mock.module("@tauri-apps/plugin-dialog", () => ({ open: async () => pickResult }));
mock.module("../hooks/useGitProgress", () => ({
  useGitProgress: () => ({ event: null, reset: () => {} }),
  formatGitBusyLabel: (base: string) => `${base}…`,
}));

const { useCloneRepo } = await import("../hooks/useCloneRepo");

beforeEach(() => {
  cloneResult = { root: "/repo" };
  cloneError = null;
  pathIsNonEmpty = false;
  cancelledIds = [];
  resolveClone = null;
  lastCloneDir = null;
  savedPrefs = [];
  pickResult = null;
});

describe("useCloneRepo", () => {
  test("it restores the folder used for the last clone", async () => {
    lastCloneDir = "/home/u/projects";
    const { result } = renderHook(() => useCloneRepo());
    await waitFor(() => expect(result.current.destination).toBe("/home/u/projects"));
  });

  test("the destination appends the repo name parsed from the url", async () => {
    lastCloneDir = "/home/u/projects";
    const { result } = renderHook(() => useCloneRepo());
    await waitFor(() => expect(result.current.destination).toBe("/home/u/projects"));

    act(() => result.current.setUrl("git@host:group/my-docs.git"));
    await waitFor(() =>
      expect(result.current.destination).toBe("/home/u/projects/my-docs"),
    );
  });

  test("submit is blocked until there is a url and a folder", async () => {
    const { result } = renderHook(() => useCloneRepo());
    expect(result.current.submitDisabled).toBe(true);

    act(() => result.current.setUrl("git@host:g/r.git"));
    act(() => result.current.setDestination("/home/u/projects/r"));
    await waitFor(() => expect(result.current.submitDisabled).toBe(false));
  });

  test("a non-empty destination blocks submit", async () => {
    pathIsNonEmpty = true;
    const { result } = renderHook(() => useCloneRepo());
    act(() => result.current.setUrl("git@host:g/r.git"));
    act(() => result.current.setDestination("/home/u/projects/r"));

    await waitFor(() => expect(result.current.conflict).toBe(true));
    expect(result.current.submitDisabled).toBe(true);
  });

  test("a successful clone hands the project to the caller", async () => {
    const opened: unknown[] = [];
    const { result } = renderHook(() => useCloneRepo((p) => opened.push(p)));
    act(() => result.current.setUrl("git@host:g/r.git"));
    act(() => result.current.setDestination("/home/u/projects/r"));

    await act(async () => {
      await result.current.submit();
    });

    expect(opened).toEqual([{ root: "/repo" }]);
    expect(result.current.cloning).toBe(false);
  });

  test("a missing SSH key is a distinct outcome, not just an error string", async () => {
    // The dialog offers a jump to settings on this one, so it cannot be
    // folded into the generic message path.
    cloneError = "no_ssh_credentials: no key configured";
    const { result } = renderHook(() => useCloneRepo());
    act(() => result.current.setUrl("git@host:g/r.git"));
    act(() => result.current.setDestination("/home/u/projects/r"));

    await act(async () => {
      await result.current.submit();
    });

    expect(result.current.needsAuth).toBe(true);
    expect(result.current.message).toContain("Аутентификация не настроена");
  });

  test("any other clone failure shows the message verbatim", async () => {
    cloneError = "repository not found";
    const { result } = renderHook(() => useCloneRepo());
    act(() => result.current.setUrl("git@host:g/r.git"));
    act(() => result.current.setDestination("/home/u/projects/r"));

    await act(async () => {
      await result.current.submit();
    });

    expect(result.current.needsAuth).toBe(false);
    expect(result.current.message).toBe("repository not found");
  });

  test("picking a folder remembers it for next time", async () => {
    pickResult = "/home/u/other";
    const { result } = renderHook(() => useCloneRepo());

    await act(async () => {
      await result.current.pickDestination();
    });

    expect(result.current.destination).toBe("/home/u/other");
    expect(savedPrefs.at(-1)).toMatchObject({ lastCloneDir: "/home/u/other" });
  });

  test("cancelling the folder picker changes nothing", async () => {
    lastCloneDir = "/home/u/projects";
    const { result } = renderHook(() => useCloneRepo());
    await waitFor(() => expect(result.current.destination).toBe("/home/u/projects"));

    await act(async () => {
      await result.current.pickDestination();
    });

    expect(result.current.destination).toBe("/home/u/projects");
    expect(savedPrefs).toHaveLength(0);
  });

  test("the destination is joined with the separator the folder already uses", async () => {
    // The Windows report this fixes: the picker hands back `C:\repos` and the
    // hook used to append `/name`, producing `C:\repos/clonned-repo`.
    lastCloneDir = "C:\\repos";
    const { result } = renderHook(() => useCloneRepo());
    await waitFor(() => expect(result.current.destination).toBe("C:\\repos"));

    act(() => result.current.setUrl("ssh://git@host/group/clonned-repo.git"));
    await waitFor(() =>
      expect(result.current.destination).toBe("C:\\repos\\clonned-repo"),
    );
  });

  test("a hand-typed Windows path is split back apart, not doubled", async () => {
    const { result } = renderHook(() => useCloneRepo());
    act(() => result.current.setUrl("git@host:g/repo.git"));
    act(() => result.current.setDestination("C:\\repos\\other\\repo"));

    await waitFor(() =>
      expect(result.current.destination).toBe("C:\\repos\\other\\repo"),
    );
  });

  test("cancelling releases the dialog and ignores the late answer", async () => {
    const opened: unknown[] = [];
    resolveClone = () => {};
    const { result } = renderHook(() => useCloneRepo((p) => opened.push(p)));
    act(() => result.current.setUrl("git@host:g/r.git"));
    act(() => result.current.setDestination("/home/u/projects/r"));

    let pending: Promise<void>;
    act(() => {
      pending = result.current.submit();
    });
    await waitFor(() => expect(result.current.cloning).toBe(true));

    act(() => result.current.cancel());
    expect(result.current.cloning).toBe(false);
    expect(cancelledIds).toHaveLength(1);

    // The abandoned clone still finishes; its result must not open a project.
    await act(async () => {
      resolveClone?.({ root: "/repo" });
      await pending;
    });
    expect(opened).toEqual([]);
  });
});
