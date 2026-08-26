import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { SkillListItem } from "../lib/skills";
import * as actualSkills from "../lib/skills";

const listed: SkillListItem[][] = [];
let listCalls = 0;
let setEnabledCalls: Array<[string, string, boolean]> = [];
let listRejectsWith: string | null = null;

function skill(name: string, enabled = true): SkillListItem {
  return { name, source: "user", description: name, enabled, error: null } as SkillListItem;
}

// The IPC wrappers are exactly the seam this layer exists to provide: thin,
// typed, and the only thing standing between the hook and Tauri.
mock.module("../lib/skills", () => ({
  ...actualSkills,
  skillsList: async () => {
    listCalls += 1;
    if (listRejectsWith) throw listRejectsWith;
    return listed[Math.min(listCalls - 1, listed.length - 1)] ?? [];
  },
  skillsSetEnabled: async (source: string, name: string, enabled: boolean) => {
    setEnabledCalls.push([source, name, enabled]);
  },
  skillsRemove: async () => {},
  skillsImport: async () => ({}),
  skillsUserDir: async () => "/tmp/skills",
}));
mock.module("@tauri-apps/plugin-dialog", () => ({ open: async () => null }));
mock.module("@tauri-apps/plugin-opener", () => ({ openPath: async () => {} }));

const { useSkills } = await import("../hooks/useSkills");

beforeEach(() => {
  listed.length = 0;
  listCalls = 0;
  setEnabledCalls = [];
  listRejectsWith = null;
});

describe("useSkills", () => {
  test("loads the list on mount", async () => {
    listed.push([skill("alpha"), skill("beta")]);
    const { result } = renderHook(() => useSkills());

    // `null` is "still loading", distinct from an empty list.
    expect(result.current.items).toBeNull();
    await waitFor(() => expect(result.current.items).not.toBeNull());
    expect(result.current.items?.map((s) => s.name)).toEqual(["alpha", "beta"]);
    expect(result.current.error).toBeNull();
  });

  test("a failing load surfaces the message instead of hanging on 'Загрузка…'", async () => {
    listRejectsWith = "skills dir unreadable";
    const { result } = renderHook(() => useSkills());

    await waitFor(() => expect(result.current.error).not.toBeNull());
    expect(result.current.error).toBe("skills dir unreadable");
    expect(result.current.items).toBeNull();
  });

  test("toggling reloads, so the list reflects what the backend now holds", async () => {
    listed.push([skill("alpha", true)], [skill("alpha", false)]);
    const { result } = renderHook(() => useSkills());
    await waitFor(() => expect(result.current.items).not.toBeNull());

    await act(async () => {
      await result.current.toggle(skill("alpha", true), false);
    });

    expect(setEnabledCalls).toEqual([["user", "alpha", false]]);
    expect(result.current.items?.[0]?.enabled).toBe(false);
    expect(result.current.busy).toBe(false);
  });

  test("a cancelled folder dialog changes nothing", async () => {
    listed.push([skill("alpha")]);
    const { result } = renderHook(() => useSkills());
    await waitFor(() => expect(result.current.items).not.toBeNull());
    const before = listCalls;

    await act(async () => {
      await result.current.addSkill();
    });

    // No import, and no pointless reload.
    expect(listCalls).toBe(before);
    expect(result.current.error).toBeNull();
  });
});
