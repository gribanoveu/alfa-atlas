import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { RecentProject } from "../lib/project";
import * as actualProject from "../lib/project";

let listResult: RecentProject[] | string = [];
let removed: string[] = [];
let removeRejectsWith: string | null = null;

mock.module("../lib/project", () => ({
  ...actualProject,
  listRecentProjects: async () => {
    if (typeof listResult === "string") throw listResult;
    return listResult;
  },
  removeRecentProject: async (root: string) => {
    removed.push(root);
    if (removeRejectsWith) throw removeRejectsWith;
    listResult = (listResult as RecentProject[]).filter((p) => p.root !== root);
  },
}));

const { useRecentProjects, useRecentProjectsList } = await import(
  "../hooks/useRecentProjects",
);

function project(root: string) {
  return { root, name: root } as RecentProject;
}

const noop = async () => {};

beforeEach(() => {
  listResult = [];
  removed = [];
  removeRejectsWith = null;
});

describe("useRecentProjects", () => {
  test("loads the recent list", async () => {
    listResult = [project("/a"), project("/b")];
    const { result } = renderHook(() =>
      useRecentProjects({ onOpenFolder: noop, onOpenRecent: noop }),
    );
    await waitFor(() => expect(result.current.recent).toHaveLength(2));
  });

  test("a failing list degrades to empty without an error", async () => {
    // The welcome screen still works without history; an error banner in
    // front of the two buttons that matter would be noise.
    listResult = "history file corrupt";
    const { result } = renderHook(() =>
      useRecentProjects({ onOpenFolder: noop, onOpenRecent: noop }),
    );
    await waitFor(() => expect(result.current.recent).toEqual([]));
    expect(result.current.error).toBeNull();
  });

  test("a failing open is surfaced, since the user asked for it", async () => {
    const { result } = renderHook(() =>
      useRecentProjects({
        onOpenFolder: noop,
        onOpenRecent: async () => {
          throw "не удалось открыть";
        },
      }),
    );

    await act(async () => {
      await result.current.openRecent("/gone");
    });

    expect(result.current.error).toBe("не удалось открыть");
    expect(result.current.busy).toBe(false);
  });

  test("a failed open refreshes the list, so it stops offering a dead entry", async () => {
    listResult = [project("/gone")];
    let listCalls = 0;
    const { result } = renderHook(() =>
      useRecentProjects({
        onOpenFolder: noop,
        onOpenRecent: async () => {
          listCalls = result.current.recent.length;
          throw "нет такой папки";
        },
      }),
    );
    await waitFor(() => expect(result.current.recent).toHaveLength(1));

    listResult = [];
    await act(async () => {
      await result.current.openRecent("/gone");
    });

    expect(listCalls).toBe(1);
    expect(result.current.recent).toEqual([]);
  });

  test("a failing folder dialog is surfaced too", async () => {
    const { result } = renderHook(() =>
      useRecentProjects({
        onOpenFolder: async () => {
          throw new Error("dialog unavailable");
        },
        onOpenRecent: noop,
      }),
    );

    await act(async () => {
      await result.current.openFolder();
    });

    expect(result.current.error).toBe("dialog unavailable");
  });

  test("removing an entry drops it from the list", async () => {
    listResult = [project("/a"), project("/b")];
    const { result } = renderHook(() =>
      useRecentProjects({ onOpenFolder: noop, onOpenRecent: noop }),
    );
    await waitFor(() => expect(result.current.recent).toHaveLength(2));

    await act(async () => {
      await result.current.removeRecent("/a");
    });

    expect(removed).toEqual(["/a"]);
    expect(result.current.recent.map((p) => p.root)).toEqual(["/b"]);
  });
});

describe("useRecentProjectsList", () => {
  test("loads the list on mount", async () => {
    listResult = [project("/a"), project("/b")];
    const { result } = renderHook(() => useRecentProjectsList());

    await waitFor(() => expect(result.current.recent).toHaveLength(2));
    expect(result.current.listError).toBeNull();
  });

  test("a failing list degrades to empty without an error", async () => {
    // Both surfaces — the welcome screen and the TopBar dropdown — still
    // work without history, so a banner in front of them would be noise.
    listResult = "history file corrupt";
    const { result } = renderHook(() => useRecentProjectsList());

    await waitFor(() => expect(result.current.recent).toEqual([]));
    expect(result.current.listError).toBeNull();
  });

  test("removing an entry drops it from the list", async () => {
    listResult = [project("/a"), project("/b")];
    const { result } = renderHook(() => useRecentProjectsList());
    await waitFor(() => expect(result.current.recent).toHaveLength(2));

    await act(async () => {
      await result.current.removeRecent("/a");
    });

    expect(removed).toEqual(["/a"]);
    await waitFor(() => expect(result.current.recent.map((p) => p.root)).toEqual(["/b"]));
  });

  test("a failing removal is surfaced, unlike a failing list", async () => {
    listResult = [project("/a")];
    removeRejectsWith = "history file read-only";
    const { result } = renderHook(() => useRecentProjectsList());
    await waitFor(() => expect(result.current.recent).toHaveLength(1));

    await act(async () => {
      await result.current.removeRecent("/a");
    });

    expect(result.current.listError).toBe("history file read-only");
    expect(result.current.recent).toHaveLength(1);
  });

  test("reload picks up entries added elsewhere", async () => {
    listResult = [project("/a")];
    const { result } = renderHook(() => useRecentProjectsList());
    await waitFor(() => expect(result.current.recent).toHaveLength(1));

    listResult = [project("/a"), project("/b")];
    await act(async () => {
      await result.current.reload();
    });

    expect(result.current.recent).toHaveLength(2);
  });
});
