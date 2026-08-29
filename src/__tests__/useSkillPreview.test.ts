import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualSkills from "../lib/skills";

let files: string[] = [];
let filesRejectsWith: string | null = null;
let readCalls: Array<[string, string, string]> = [];
const contents: Record<string, string> = {};

mock.module("../lib/skills", () => ({
  ...actualSkills,
  skillsFiles: async () => {
    if (filesRejectsWith) throw filesRejectsWith;
    return files;
  },
  skillsReadFile: async (source: string, name: string, path: string) => {
    readCalls.push([source, name, path]);
    const text = contents[path];
    if (text === undefined) throw `no such file: ${path}`;
    return text;
  },
}));

const { useSkillPreview } = await import("../hooks/useSkillPreview");

beforeEach(() => {
  files = [];
  filesRejectsWith = null;
  readCalls = [];
  for (const key of Object.keys(contents)) delete contents[key];
});

describe("useSkillPreview", () => {
  test("opens SKILL.md — the first file — without being asked", async () => {
    files = ["SKILL.md", "references/structure.md"];
    contents["SKILL.md"] = "# Skill";
    const { result } = renderHook(() => useSkillPreview("bundled", "method-spec"));

    await waitFor(() => expect(result.current.content).not.toBeNull());
    expect(result.current.files).toEqual(["SKILL.md", "references/structure.md"]);
    expect(result.current.selected).toBe("SKILL.md");
    expect(readCalls).toEqual([["bundled", "method-spec", "SKILL.md"]]);
  });

  test("selecting another file loads it", async () => {
    files = ["SKILL.md", "references/structure.md"];
    contents["SKILL.md"] = "# Skill";
    contents["references/structure.md"] = "# Structure";
    const { result } = renderHook(() => useSkillPreview("user", "my-skill"));
    await waitFor(() => expect(result.current.content).toBe("# Skill"));

    act(() => result.current.select("references/structure.md"));

    await waitFor(() => expect(result.current.content).toBe("# Structure"));
    expect(result.current.error).toBeNull();
  });

  test("a skill with no readable files reports the failure instead of hanging", async () => {
    filesRejectsWith = "skill not found: ghost";
    const { result } = renderHook(() => useSkillPreview("user", "ghost"));

    await waitFor(() => expect(result.current.error).toBe("skill not found: ghost"));
    // Not `null`, so the viewer shows "Нет файлов" rather than "Загрузка…".
    expect(result.current.files).toEqual([]);
    expect(result.current.selected).toBeNull();
  });

  test("a failing read leaves the pane empty and surfaces the message", async () => {
    files = ["SKILL.md"];
    const { result } = renderHook(() => useSkillPreview("user", "broken"));

    await waitFor(() => expect(result.current.error).not.toBeNull());
    expect(result.current.content).toBeNull();
    expect(result.current.loadingContent).toBe(false);
  });
});
