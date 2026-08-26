import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualSearch from "../lib/search";
import type { DocsSearchResults } from "../lib/search";

type Args = { pattern: string; glob: string | null; caseInsensitive: boolean };

let calls: Array<[string, Args]> = [];
let throwsWith: string | null = null;
let pending: Array<(r: DocsSearchResults) => void> = [];
let deferNext = false;

mock.module("../lib/search", () => ({
  ...actualSearch,
  docsSearch: (root: string, args: Args) => {
    calls.push([root, args]);
    if (throwsWith) return Promise.reject(throwsWith);
    if (deferNext) {
      return new Promise<DocsSearchResults>((resolve) => pending.push(resolve));
    }
    return Promise.resolve({ files: [], totalMatches: 0 } as unknown as DocsSearchResults);
  },
}));

const { useDocsSearch } = await import("../hooks/useDocsSearch");


/** Lets the in-flight request settle inside `act`, so the state update it
 * makes does not land after the test has already finished. */
async function settle(result: { current: { loading: boolean } }) {
  await waitFor(() => expect(result.current.loading).toBe(false));
}

beforeEach(() => {
  calls = [];
  throwsWith = null;
  pending = [];
  deferNext = false;
});

describe("useDocsSearch", () => {
  test("typing does not search until the debounce elapses", async () => {
    const { result } = renderHook(() => useDocsSearch("/repo/docs"));

    act(() => result.current.setQuery("метод"));
    expect(calls).toHaveLength(0);

    await waitFor(() => expect(calls).toHaveLength(1), { timeout: 1000 });
    await settle(result);
  });

  test("a literal query is escaped so it is not read as a regex", async () => {
    // Searching for `a.b` must not match `axb`.
    const { result } = renderHook(() => useDocsSearch("/repo/docs"));
    act(() => result.current.setQuery("a.b(c)"));

    await waitFor(() => expect(calls).toHaveLength(1), { timeout: 1000 });
    expect(calls[0]?.[1].pattern).toBe("a\\.b\\(c\\)");
    await settle(result);
  });

  test("regex mode passes the query through untouched", async () => {
    const { result } = renderHook(() => useDocsSearch("/repo/docs"));
    act(() => result.current.setUseRegex(true));
    act(() => result.current.setQuery("a.b(c)"));

    await waitFor(() => expect(calls).toHaveLength(1), { timeout: 1000 });
    expect(calls[0]?.[1].pattern).toBe("a.b(c)");
    await settle(result);
  });

  test("match-case is sent as its inverse, since the backend takes caseInsensitive", async () => {
    const { result } = renderHook(() => useDocsSearch("/repo/docs"));
    act(() => result.current.setQuery("метод"));
    await waitFor(() => expect(calls).toHaveLength(1), { timeout: 1000 });
    expect(calls[0]?.[1].caseInsensitive).toBe(true);

    act(() => result.current.setMatchCase(true));
    await waitFor(() => expect(calls).toHaveLength(2), { timeout: 1000 });
    expect(calls[1]?.[1].caseInsensitive).toBe(false);
    await settle(result);
  });

  test("a blank glob is sent as null rather than an empty filter", async () => {
    const { result } = renderHook(() => useDocsSearch("/repo/docs"));
    act(() => result.current.setQuery("метод"));
    await waitFor(() => expect(calls).toHaveLength(1), { timeout: 1000 });
    expect(calls[0]?.[1].glob).toBeNull();

    act(() => result.current.setGlob("  *.adoc  "));
    await waitFor(() => expect(calls).toHaveLength(2), { timeout: 1000 });
    expect(calls[1]?.[1].glob).toBe("*.adoc");
    await settle(result);
  });

  test("a whitespace-only query never reaches the backend", async () => {
    const { result } = renderHook(() => useDocsSearch("/repo/docs"));
    act(() => result.current.setQuery("   "));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 400));
    });
    expect(calls).toHaveLength(0);
    expect(result.current.results).toBeNull();
  });

  test("no docs root means no search", async () => {
    const { result } = renderHook(() => useDocsSearch(null));
    act(() => result.current.setQuery("метод"));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 400));
    });
    expect(calls).toHaveLength(0);
  });

  test("searchNow skips the debounce", async () => {
    const { result } = renderHook(() => useDocsSearch("/repo/docs"));
    act(() => result.current.setQuery("метод"));

    act(() => result.current.searchNow());
    expect(calls).toHaveLength(1);
    await settle(result);
  });

  test("a slow response that lost the race is discarded", async () => {
    // Otherwise results for an abandoned query would land on screen after
    // the user has already typed something else.
    deferNext = true;
    const { result } = renderHook(() => useDocsSearch("/repo/docs"));
    act(() => result.current.setQuery("первый"));
    act(() => result.current.searchNow());
    act(() => result.current.setQuery("второй"));
    act(() => result.current.searchNow());
    expect(pending).toHaveLength(2);

    const stale = { files: [{ path: "stale.adoc" }], totalMatches: 1 } as unknown as DocsSearchResults;
    await act(async () => {
      pending[0]?.(stale);
    });

    expect(result.current.results).toBeNull();
  });

  test("a failing search clears results and reports why", async () => {
    throwsWith = "invalid regex";
    const { result } = renderHook(() => useDocsSearch("/repo/docs"));
    act(() => result.current.setQuery("["));
    act(() => result.current.searchNow());

    await waitFor(() => expect(result.current.error).toBe("invalid regex"));
    expect(result.current.results).toBeNull();
    expect(result.current.loading).toBe(false);
  });

  test("reset clears the query, the filter and the results", async () => {
    const { result } = renderHook(() => useDocsSearch("/repo/docs"));
    act(() => result.current.setQuery("метод"));
    act(() => result.current.setGlob("*.adoc"));
    await waitFor(() => expect(calls).toHaveLength(1), { timeout: 1000 });

    act(() => result.current.reset());
    expect(result.current.query).toBe("");
    expect(result.current.glob).toBe("");
    expect(result.current.results).toBeNull();
    expect(result.current.loading).toBe(false);
  });

  test("a response arriving after reset is ignored", async () => {
    deferNext = true;
    const { result } = renderHook(() => useDocsSearch("/repo/docs"));
    act(() => result.current.setQuery("метод"));
    act(() => result.current.searchNow());

    act(() => result.current.reset());
    await act(async () => {
      pending[0]?.({ files: [{ path: "late.adoc" }], totalMatches: 1 } as unknown as DocsSearchResults);
    });

    expect(result.current.results).toBeNull();
  });
});
