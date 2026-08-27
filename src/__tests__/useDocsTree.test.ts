import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualProject from "../lib/project";
import type { TreeNode } from "../lib/project";

let tree: TreeNode[] = [];
let throwsWith: string | null = null;
let calls = 0;
let deferNext = false;
let pending: Array<(t: TreeNode[]) => void> = [];

mock.module("../lib/project", () => ({
  ...actualProject,
  listDocsTree: (_root: string) => {
    calls += 1;
    if (throwsWith) return Promise.reject(throwsWith);
    if (deferNext) return new Promise<TreeNode[]>((r) => pending.push(r));
    return Promise.resolve(tree);
  },
}));

const { useDocsTree } = await import("../hooks/useDocsTree");

function node(path: string): TreeNode {
  return { path, name: path, isDir: false } as TreeNode;
}

beforeEach(() => {
  tree = [node("a.adoc")];
  throwsWith = null;
  calls = 0;
  deferNext = false;
  pending = [];
});

describe("useDocsTree", () => {
  test("loads the tree for a docs root", async () => {
    const { result } = renderHook(() => useDocsTree("/repo/docs"));
    await waitFor(() => expect(result.current.nodes).toHaveLength(1));
    expect(result.current.error).toBeNull();
  });

  test("no docs root means an empty tree and no call", async () => {
    const { result } = renderHook(() => useDocsTree(null));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.nodes).toEqual([]);
    expect(calls).toBe(0);
  });

  test("the first load shows a spinner", async () => {
    deferNext = true;
    const { result } = renderHook(() => useDocsTree("/repo/docs"));
    await waitFor(() => expect(result.current.loading).toBe(true));

    await act(async () => {
      pending[0]?.([node("a.adoc")]);
    });
    expect(result.current.loading).toBe(false);
  });

  test("a later refresh does not flash the spinner", async () => {
    // The tree is already on screen; blanking it to a spinner on every
    // file-watcher tick would make the sidebar flicker.
    const { result } = renderHook(() => useDocsTree("/repo/docs"));
    await waitFor(() => expect(result.current.nodes).toHaveLength(1));

    deferNext = true;
    act(() => {
      void result.current.refresh();
    });
    expect(result.current.loading).toBe(false);

    await act(async () => {
      pending[0]?.([node("a.adoc"), node("b.adoc")]);
    });
    expect(result.current.nodes).toHaveLength(2);
  });

  test("switching project reloads and shows the spinner again", async () => {
    const { result, rerender } = renderHook(({ root }) => useDocsTree(root), {
      initialProps: { root: "/repo/docs" as string | null },
    });
    await waitFor(() => expect(result.current.nodes).toHaveLength(1));

    deferNext = true;
    rerender({ root: "/other/docs" });
    await waitFor(() => expect(result.current.loading).toBe(true));

    await act(async () => {
      pending.at(-1)?.([node("x.adoc")]);
    });
    expect(result.current.nodes).toEqual([node("x.adoc")]);
  });

  test("a failing load clears the tree and reports why", async () => {
    throwsWith = "docs root missing";
    const { result } = renderHook(() => useDocsTree("/repo/docs"));

    await waitFor(() => expect(result.current.error).toBe("docs root missing"));
    expect(result.current.nodes).toEqual([]);
    expect(result.current.loading).toBe(false);
  });

  test("a successful refresh clears an earlier error", async () => {
    throwsWith = "docs root missing";
    const { result } = renderHook(() => useDocsTree("/repo/docs"));
    await waitFor(() => expect(result.current.error).not.toBeNull());

    throwsWith = null;
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.error).toBeNull();
    expect(result.current.nodes).toHaveLength(1);
  });
});
