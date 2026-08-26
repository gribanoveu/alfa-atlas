import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualAiTools from "../lib/aiTools";

let autoApprovedResult: string[] | Error | string = [];
let allowedResult: string[] | Error | string = [];
let setAllowedCalls: Array<[string, boolean]> = [];
let setAutoCalls: Array<[string, boolean]> = [];
let failNextWrite: string | null = null;

async function resolveOrThrow(value: string[] | Error | string) {
  if (Array.isArray(value)) return value;
  throw value;
}

mock.module("../lib/aiTools", () => ({
  ...actualAiTools,
  getAutoApprovedTools: () => resolveOrThrow(autoApprovedResult),
  getAllowedTools: () => resolveOrThrow(allowedResult),
  setToolAutoApproved: async (tool: string, on: boolean) => {
    setAutoCalls.push([tool, on]);
    if (failNextWrite) throw failNextWrite;
  },
  setToolAllowed: async (tool: string, on: boolean) => {
    setAllowedCalls.push([tool, on]);
    if (failNextWrite) throw failNextWrite;
  },
}));

const { useToolPermissions } = await import("../hooks/useToolPermissions");

beforeEach(() => {
  autoApprovedResult = [];
  allowedResult = [];
  setAllowedCalls = [];
  setAutoCalls = [];
  failNextWrite = null;
});

describe("useToolPermissions", () => {
  test("loads both lists independently", async () => {
    autoApprovedResult = ["writeFile"];
    allowedResult = ["readFile", "grep"];
    const { result } = renderHook(() => useToolPermissions());

    await waitFor(() => expect(result.current.allowed.loading).toBe(false));
    expect(result.current.autoApproved.tools).toEqual(["writeFile"]);
    expect(result.current.allowed.tools).toEqual(["readFile", "grep"]);
  });

  test("'no project is open' is a state, not an error banner", async () => {
    // This tab is the only project-scoped one, so this failure happens on a
    // perfectly healthy install and must not look like a fault.
    autoApprovedResult = "no project is open";
    allowedResult = "no project is open";
    const { result } = renderHook(() => useToolPermissions());

    await waitFor(() => expect(result.current.allowed.loading).toBe(false));
    expect(result.current.autoApproved.noProject).toBe(true);
    expect(result.current.autoApproved.error).toBeNull();
    expect(result.current.allowed.noProject).toBe(true);
    expect(result.current.allowed.error).toBeNull();
  });

  test("any other failure is surfaced as an error", async () => {
    autoApprovedResult = "config file is corrupt";
    const { result } = renderHook(() => useToolPermissions());

    await waitFor(() => expect(result.current.autoApproved.loading).toBe(false));
    expect(result.current.autoApproved.error).toBe("config file is corrupt");
    expect(result.current.autoApproved.noProject).toBe(false);
  });

  test("one list failing leaves the other intact", async () => {
    autoApprovedResult = "boom";
    allowedResult = ["readFile"];
    const { result } = renderHook(() => useToolPermissions());

    await waitFor(() => expect(result.current.allowed.loading).toBe(false));
    expect(result.current.autoApproved.error).toBe("boom");
    expect(result.current.allowed.tools).toEqual(["readFile"]);
    expect(result.current.allowed.error).toBeNull();
  });

  test("revoking removes the tool from the standing-approval list", async () => {
    autoApprovedResult = ["writeFile", "deleteFile"];
    const { result } = renderHook(() => useToolPermissions());
    await waitFor(() => expect(result.current.autoApproved.loading).toBe(false));

    await act(async () => {
      await result.current.revokeAutoApproval("writeFile");
    });

    expect(setAutoCalls).toEqual([["writeFile", false]]);
    expect(result.current.autoApproved.tools).toEqual(["deleteFile"]);
    expect(result.current.autoApproved.pending).toBeNull();
  });

  test("toggling allowed adds and removes", async () => {
    allowedResult = ["readFile"];
    const { result } = renderHook(() => useToolPermissions());
    await waitFor(() => expect(result.current.allowed.loading).toBe(false));

    await act(async () => {
      await result.current.toggleAllowed("grep", true);
    });
    expect(result.current.allowed.tools).toEqual(["readFile", "grep"]);

    await act(async () => {
      await result.current.toggleAllowed("readFile", false);
    });
    expect(result.current.allowed.tools).toEqual(["grep"]);
    expect(setAllowedCalls).toEqual([["grep", true], ["readFile", false]]);
  });

  test("a failed write leaves the list untouched and reports why", async () => {
    allowedResult = ["readFile"];
    failNextWrite = "permission denied";
    const { result } = renderHook(() => useToolPermissions());
    await waitFor(() => expect(result.current.allowed.loading).toBe(false));

    await act(async () => {
      await result.current.toggleAllowed("grep", true);
    });

    // The row must not appear enabled when the write never landed.
    expect(result.current.allowed.tools).toEqual(["readFile"]);
    expect(result.current.allowed.error).toBe("permission denied");
    expect(result.current.allowed.pending).toBeNull();
  });
});
