import { beforeEach, describe, expect, test } from "bun:test";
import { mock } from "bun:test";
import { renderHook, waitFor } from "@testing-library/react";
import * as actualAiTools from "../lib/aiTools";

/** The panel mounts with no project open and never remounts, so both of
 * these have to re-read when one is opened later — otherwise the assistant
 * keeps being told there is no project and the access toggle stays
 * disabled for the rest of the session. */

let modeCalls = 0;
let definitionCalls = 0;

mock.module("../lib/aiTools", () => ({
  ...actualAiTools,
  getAiAccessMode: async () => {
    modeCalls += 1;
    return "fullRepo";
  },
  getToolDefinitions: async () => {
    definitionCalls += 1;
    return [{ name: "readFile", description: "", parameters: {} }];
  },
}));

const { useAiAccessMode } = await import("../hooks/useAiAccessMode");
const { useToolDefinitions } = await import("../hooks/useToolDefinitions");

beforeEach(() => {
  modeCalls = 0;
  definitionCalls = 0;
});

describe("assistant refetches when a project is opened mid-session", () => {
  test("access mode is read once the project arrives", async () => {
    const { result, rerender } = renderHook(
      ({ root }: { root: string | null }) => useAiAccessMode(root),
      { initialProps: { root: null } },
    );

    await waitFor(() => expect(result.current.mode).toBeNull());
    expect(modeCalls).toBe(0);

    rerender({ root: "/repo" });
    await waitFor(() => expect(result.current.mode).toBe("fullRepo"));
    expect(modeCalls).toBe(1);
  });

  test("tool definitions are refetched even when the access mode is unchanged", async () => {
    const { rerender } = renderHook(
      ({ root }: { root: string | null }) =>
        useToolDefinitions("docsOnly", "agent", root),
      { initialProps: { root: null } },
    );

    await waitFor(() => expect(definitionCalls).toBe(1));

    rerender({ root: "/repo" });
    await waitFor(() => expect(definitionCalls).toBe(2));
  });
});
