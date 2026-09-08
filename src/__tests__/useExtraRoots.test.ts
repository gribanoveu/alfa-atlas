import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualAiTools from "../lib/aiTools";

type Root = { name: string; path: string };

let listResult: Root[] | Error | string = [];
let suggestResult: Root[] = [];
let suggestCalls = 0;
let addCalls: Array<[string, string]> = [];
let removeCalls: string[] = [];
let failNextWrite: string | null = null;

async function resolveOrThrow(value: Root[] | Error | string) {
  if (Array.isArray(value)) return value;
  throw value;
}

mock.module("../lib/aiTools", () => ({
  ...actualAiTools,
  getExtraRoots: () => resolveOrThrow(listResult),
  suggestExtraRoots: async () => {
    suggestCalls += 1;
    return suggestResult;
  },
  addExtraRoot: async (name: string, path: string) => {
    addCalls.push([name, path]);
    if (failNextWrite) throw failNextWrite;
  },
  removeExtraRoot: async (name: string) => {
    removeCalls.push(name);
    if (failNextWrite) throw failNextWrite;
  },
}));

const { useExtraRoots, deriveRootName } = await import("../hooks/useExtraRoots");

beforeEach(() => {
  listResult = [];
  suggestResult = [];
  suggestCalls = 0;
  addCalls = [];
  removeCalls = [];
  failNextWrite = null;
});

describe("deriveRootName", () => {
  test("uses the folder's own name", () => {
    expect(deriveRootName("/home/u/.m2/repository/com/acme/client", [])).toBe("client");
    expect(deriveRootName("/home/u/node_modules/lodash/", [])).toBe("lodash");
  });

  test("replaces what cannot be one path segment", () => {
    expect(deriveRootName("/tmp/spring boot@5", [])).toBe("spring-boot-5");
    // A leading dot would be rejected by the Rust-side name check.
    expect(deriveRootName("/tmp/.hidden", [])).toBe("hidden");
    expect(deriveRootName("/", [])).toBe("source");
  });

  test("steps around a name already taken", () => {
    expect(deriveRootName("/a/client", ["client"])).toBe("client-2");
    expect(deriveRootName("/a/client", ["client", "client-2"])).toBe("client-3");
  });
});

describe("useExtraRoots", () => {
  test("loads the project's roots", async () => {
    listResult = [{ name: "acme", path: "/deps/acme" }];
    const { result } = renderHook(() => useExtraRoots());

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.roots).toEqual([{ name: "acme", path: "/deps/acme" }]);
    expect(result.current.noProject).toBe(false);
  });

  test("treats a closed project as a state, not an error", async () => {
    listResult = "no project is open";
    const { result } = renderHook(() => useExtraRoots());

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.noProject).toBe(true);
    expect(result.current.error).toBeNull();
  });

  test("adds a picked folder under a name derived from it", async () => {
    listResult = [{ name: "client", path: "/deps/one/client" }];
    const { result } = renderHook(() => useExtraRoots());
    await waitFor(() => expect(result.current.loading).toBe(false));

    await act(async () => {
      await result.current.add("/deps/two/client");
    });

    // The name already in use is stepped around rather than colliding.
    expect(addCalls).toEqual([["client-2", "/deps/two/client"]]);
    expect(result.current.roots.map((r) => r.name)).toEqual(["client", "client-2"]);
  });

  test("surfaces a rejected add and leaves the list alone", async () => {
    const { result } = renderHook(() => useExtraRoots());
    await waitFor(() => expect(result.current.loading).toBe(false));
    failNextWrite = "источник с именем «client» уже добавлен";

    await act(async () => {
      await result.current.add("/deps/client");
    });

    expect(result.current.error).toBe("источник с именем «client» уже добавлен");
    expect(result.current.roots).toEqual([]);
    expect(result.current.pending).toBeNull();
  });

  test("offers what the project has but has not added", async () => {
    suggestResult = [{ name: "node_modules", path: "/repo/node_modules" }];
    const { result } = renderHook(() => useExtraRoots());

    await waitFor(() => expect(result.current.suggestions).toHaveLength(1));
    expect(result.current.suggestions[0].name).toBe("node_modules");
  });

  test("adds a suggestion under its own name, then re-reads the offers", async () => {
    suggestResult = [{ name: "node_modules", path: "/repo/node_modules" }];
    const { result } = renderHook(() => useExtraRoots());
    await waitFor(() => expect(result.current.suggestions).toHaveLength(1));
    const before = suggestCalls;

    await act(async () => {
      await result.current.addSuggested(result.current.suggestions[0]);
    });

    // Not `deriveRootName` — a detected root already carries its name.
    expect(addCalls).toEqual([["node_modules", "/repo/node_modules"]]);
    expect(suggestCalls).toBeGreaterThan(before);
  });

  test("removes a root", async () => {
    listResult = [
      { name: "acme", path: "/deps/acme" },
      { name: "other", path: "/deps/other" },
    ];
    const { result } = renderHook(() => useExtraRoots());
    await waitFor(() => expect(result.current.loading).toBe(false));

    await act(async () => {
      await result.current.remove("acme");
    });

    expect(removeCalls).toEqual(["acme"]);
    expect(result.current.roots.map((r) => r.name)).toEqual(["other"]);
  });
});
