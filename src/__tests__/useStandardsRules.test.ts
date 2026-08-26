import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { RuleDef, StandardsRuleConfig } from "../lib/standards";
import * as actualStandards from "../lib/standards";

const RULES = [
  { id: "has-purpose", defaultEnabled: true },
  { id: "has-examples", defaultEnabled: false },
] as unknown as RuleDef[];

let stored: StandardsRuleConfig;
let writeFails: string | null = null;
let writes: StandardsRuleConfig[] = [];

mock.module("../lib/standards", () => ({
  ...actualStandards,
  getStandardsRules: async () => RULES,
  getStandardsConfig: async () => stored,
  setStandardsConfig: async (next: StandardsRuleConfig) => {
    if (writeFails) throw writeFails;
    writes.push(next);
    stored = next;
  },
}));

const { useStandardsRules } = await import("../hooks/useStandardsRules");

beforeEach(() => {
  stored = { rules: {} };
  writeFails = null;
  writes = [];
});

describe("useStandardsRules", () => {
  test("a rule the config says nothing about falls back to its own default", async () => {
    // A newly shipped rule is on until the user turns it off — falling back
    // to `false` would silently disable every rule added after a user last
    // touched this screen.
    const { result } = renderHook(() => useStandardsRules());
    await waitFor(() => expect(result.current.rules).not.toBeNull());

    expect(result.current.isEnabled(RULES[0]!)).toBe(true);
    expect(result.current.isEnabled(RULES[1]!)).toBe(false);
  });

  test("an explicit config value wins over the default", async () => {
    stored = { rules: { "has-purpose": false, "has-examples": true } };
    const { result } = renderHook(() => useStandardsRules());
    await waitFor(() => expect(result.current.rules).not.toBeNull());

    expect(result.current.isEnabled(RULES[0]!)).toBe(false);
    expect(result.current.isEnabled(RULES[1]!)).toBe(true);
  });

  test("toggling writes only that rule and leaves the others alone", async () => {
    stored = { rules: { "has-examples": true } };
    const { result } = renderHook(() => useStandardsRules());
    await waitFor(() => expect(result.current.rules).not.toBeNull());

    await act(async () => {
      await result.current.toggleRule(RULES[0]!, false);
    });

    expect(writes.at(-1)?.rules).toEqual({ "has-examples": true, "has-purpose": false });
    expect(result.current.isEnabled(RULES[0]!)).toBe(false);
  });

  test("a failed write rolls the checkbox back", async () => {
    stored = { rules: { "has-purpose": true } };
    const { result } = renderHook(() => useStandardsRules());
    await waitFor(() => expect(result.current.rules).not.toBeNull());
    writeFails = "read-only config";

    await act(async () => {
      await result.current.toggleRule(RULES[0]!, false);
    });

    expect(result.current.isEnabled(RULES[0]!)).toBe(true);
    expect(result.current.error).toBe("read-only config");
    expect(result.current.busy).toBe(false);
  });
});
