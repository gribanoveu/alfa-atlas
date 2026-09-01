import { describe, expect, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import { useGitFileDiff } from "../hooks/useGitFileDiff";
import type { GitDiffScope, GitFileDiff, GitFileStatus } from "../lib/git";

// No module mocks here: the loaders arrive as arguments, which is the whole
// point of the hook — it owns the state, `App` owns the IPC. They are held
// in consts rather than built inside the render callback because the hook
// reloads whenever `onLoadDiff` changes identity, and `App` supplies
// `useCallback`-stable ones.
function target(path = "docs/a.adoc", scope: GitDiffScope = "worktree") {
  return { file: { path } as GitFileStatus, scope };
}

function diff(original: string, modified: string): GitFileDiff {
  return { original, modified, isBinary: false } as GitFileDiff;
}

type Deps = Parameters<typeof useGitFileDiff>[0];

function deps(over: Partial<Deps> = {}): Deps {
  return {
    target: target(),
    onLoadDiff: async () => diff("a", "b"),
    onDiscard: async () => true,
    onSaveContent: async () => true,
    ...over,
  };
}

describe("useGitFileDiff", () => {
  test("loads the diff on mount", async () => {
    const seen: Array<[string, GitDiffScope]> = [];
    const d = deps({
      onLoadDiff: async (path, scope) => {
        seen.push([path, scope]);
        return diff("old", "new");
      },
    });
    const { result } = renderHook(() => useGitFileDiff(d));

    expect(result.current.loading).toBe(true);
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.diff?.modified).toBe("new");
    expect(result.current.error).toBeNull();
    expect(seen).toEqual([["docs/a.adoc", "worktree"]]);
  });

  test("a missing diff is an error, not an empty view", async () => {
    const d = deps({ onLoadDiff: async () => null });
    const { result } = renderHook(() => useGitFileDiff(d));

    await waitFor(() => expect(result.current.error).toBe("Не удалось загрузить diff"));
    expect(result.current.diff).toBeNull();
    expect(result.current.loading).toBe(false);
  });

  test("switching target reloads and blanks the previous diff first", async () => {
    const seen: string[] = [];
    const onLoadDiff = async (path: string) => {
      seen.push(path);
      return diff(path, path);
    };
    const { result, rerender } = renderHook(
      (props: { path: string }) =>
        useGitFileDiff(deps({ target: target(props.path), onLoadDiff })),
      { initialProps: { path: "docs/a.adoc" } },
    );
    await waitFor(() => expect(result.current.diff?.original).toBe("docs/a.adoc"));

    rerender({ path: "docs/b.adoc" });
    // The old file's diff must not linger under the new file's header.
    expect(result.current.diff).toBeNull();
    await waitFor(() => expect(result.current.diff?.original).toBe("docs/b.adoc"));
    expect(seen).toEqual(["docs/a.adoc", "docs/b.adoc"]);
  });

  test("switching scope reloads too", async () => {
    const seen: GitDiffScope[] = [];
    const onLoadDiff = async (_path: string, scope: GitDiffScope) => {
      seen.push(scope);
      return diff("x", "y");
    };
    const { result, rerender } = renderHook(
      (props: { scope: GitDiffScope }) =>
        useGitFileDiff(deps({ target: target("docs/a.adoc", props.scope), onLoadDiff })),
      { initialProps: { scope: "worktree" as GitDiffScope } },
    );
    await waitFor(() => expect(result.current.loading).toBe(false));

    rerender({ scope: "index" as GitDiffScope });
    await waitFor(() => expect(seen).toEqual(["worktree", "index"]));
  });

  test("a discard that lands reports true so the caller can close", async () => {
    const discarded: string[] = [];
    const d = deps({
      onDiscard: async (path) => {
        discarded.push(path);
        return true;
      },
    });
    const { result } = renderHook(() => useGitFileDiff(d));
    await waitFor(() => expect(result.current.loading).toBe(false));

    let ok: boolean | undefined;
    await act(async () => {
      ok = await result.current.discard();
    });

    expect(ok).toBe(true);
    expect(discarded).toEqual(["docs/a.adoc"]);
    expect(result.current.discarding).toBe(false);
    expect(result.current.error).toBeNull();
  });

  test("a refused discard keeps the modal open with a message", async () => {
    const d = deps({ onDiscard: async () => false });
    const { result } = renderHook(() => useGitFileDiff(d));
    await waitFor(() => expect(result.current.loading).toBe(false));

    let ok: boolean | undefined;
    await act(async () => {
      ok = await result.current.discard();
    });

    expect(ok).toBe(false);
    expect(result.current.error).toBe("Не удалось отменить изменения");
    expect(result.current.discarding).toBe(false);
  });

  test("a save reloads the diff, so reverted hunks stop showing as changes", async () => {
    const saved: string[] = [];
    let loads = 0;
    const d = deps({
      onLoadDiff: async () => {
        loads += 1;
        return diff("old", loads === 1 ? "new" : "saved");
      },
      onSaveContent: async (_path, _scope, content) => {
        saved.push(content);
        return true;
      },
    });
    const { result } = renderHook(() => useGitFileDiff(d));
    await waitFor(() => expect(result.current.diff?.modified).toBe("new"));

    let outcome: string | undefined;
    await act(async () => {
      outcome = await result.current.save("edited");
    });

    expect(outcome).toBe("saved");
    expect(saved).toEqual(["edited"]);
    expect(result.current.diff?.modified).toBe("saved");
    expect(result.current.saving).toBe(false);
  });

  test('a save that leaves an empty diff reports "gone"', async () => {
    let loads = 0;
    const d = deps({
      onLoadDiff: async () => {
        loads += 1;
        return loads === 1 ? diff("old", "new") : diff("same", "same");
      },
    });
    const { result } = renderHook(() => useGitFileDiff(d));
    await waitFor(() => expect(result.current.diff?.modified).toBe("new"));

    // The file now matches HEAD. The real git command returns an object with
    // equal sides, so the caller must close rather than keep the modal open.
    let outcome: string | undefined;
    await act(async () => {
      outcome = await result.current.save("reverted");
    });

    expect(outcome).toBe("gone");
    expect(result.current.error).toBeNull();
  });

  test('a refused save reports "failed" and never reloads', async () => {
    let loads = 0;
    const d = deps({
      onLoadDiff: async () => {
        loads += 1;
        return diff("old", "new");
      },
      onSaveContent: async () => false,
    });
    const { result } = renderHook(() => useGitFileDiff(d));
    await waitFor(() => expect(result.current.loading).toBe(false));

    let outcome: string | undefined;
    await act(async () => {
      outcome = await result.current.save("edited");
    });

    expect(outcome).toBe("failed");
    expect(result.current.error).toBe("Не удалось сохранить изменения");
    // Reloading after a failed write would replace the user's unsaved edits
    // with the on-disk content they were trying to overwrite.
    expect(loads).toBe(1);
    expect(result.current.saving).toBe(false);
  });

  test("a save clears the error left by a previous failure", async () => {
    let allowSave = false;
    const d = deps({ onSaveContent: async () => allowSave });
    const { result } = renderHook(() => useGitFileDiff(d));
    await waitFor(() => expect(result.current.loading).toBe(false));

    await act(async () => {
      await result.current.save("edited");
    });
    expect(result.current.error).not.toBeNull();

    allowSave = true;
    await act(async () => {
      await result.current.save("edited again");
    });
    expect(result.current.error).toBeNull();
  });

  test("state that lands after unmount is dropped", async () => {
    let release: (() => void) | null = null;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    const d = deps({
      onDiscard: async () => {
        await held;
        return false;
      },
    });
    const { result, unmount } = renderHook(() => useGitFileDiff(d));
    await waitFor(() => expect(result.current.loading).toBe(false));

    let pending: Promise<boolean> | undefined;
    act(() => {
      pending = result.current.discard();
    });
    unmount();

    // Setting state into an unmounted hook would be a React warning at best.
    await act(async () => {
      release!();
      expect(await pending).toBe(false);
    });
  });
});
