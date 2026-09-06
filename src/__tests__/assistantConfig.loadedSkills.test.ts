import { describe, expect, test } from "bun:test";
import {
  buildLoadedSkillsContextBlock,
  LOADED_SKILL_CONTEXT_CHARS,
} from "../lib/assistantConfig";
import type { ChatMessage, MessageBlock, ToolCallBlock } from "../lib/chatBlocks";

function loadCall(
  name: string,
  body: string,
  status: ToolCallBlock["status"] = "done",
  source: "bundled" | "user" = "bundled",
): MessageBlock {
  return {
    type: "toolCall",
    id: `load_${name}_${body.length}`,
    name: "skill",
    argumentsJson: JSON.stringify({ op: "load", name }),
    status,
    ...(status === "done"
      ? { result: { tool: "skillLoaded" as const, result: { name, source, body, files: [] } } }
      : {}),
  } as MessageBlock;
}

function readCall(name: string, path: string, content: string): MessageBlock {
  return {
    type: "toolCall",
    id: `read_${name}_${path}`,
    name: "skill",
    argumentsJson: JSON.stringify({ op: "read", name, path }),
    status: "done",
    result: { tool: "skillFile", result: { name, path, content } },
  } as MessageBlock;
}

function turn(...blocks: MessageBlock[]): ChatMessage {
  return { id: `m${blocks.length}${Math.random()}`, role: "assistant", blocks, streaming: false };
}

const USER: ChatMessage = { id: "u1", role: "user", content: "составь тикет" };

describe("buildLoadedSkillsContextBlock", () => {
  test("says nothing when no skill was ever loaded", () => {
    expect(buildLoadedSkillsContextBlock([])).toBeNull();
    expect(
      buildLoadedSkillsContextBlock([
        USER,
        turn({ type: "text", id: "t", content: "Готово." }),
      ]),
    ).toBeNull();
  });

  // A call still awaiting approval or one that failed carries no body —
  // announcing a skill as loaded on the strength of it would be a lie.
  test("ignores calls that never settled", () => {
    expect(
      buildLoadedSkillsContextBlock([turn(loadCall("method-spec", "RULES", "running"))]),
    ).toBeNull();
    expect(
      buildLoadedSkillsContextBlock([turn(loadCall("method-spec", "RULES", "error"))]),
    ).toBeNull();
  });

  test("reproduces the loaded skill in full, framed as instructions", () => {
    const block = buildLoadedSkillsContextBlock([
      USER,
      turn(loadCall("jira-task-description", "## Acceptance Criteria\nWrite them.")),
    ]);
    expect(block).toContain("[Skill]");
    expect(block).toContain("jira-task-description");
    expect(block).toContain("--- SKILL `jira-task-description` (bundled) ---");
    expect(block).toContain("## Acceptance Criteria\nWrite them.");
    expect(block).toContain("--- END SKILL `jira-task-description` ---");
  });

  // The most recent load is the one the current work is about; an older
  // skill is a pointer, not 42 KB of text the model has moved on from.
  test("keeps only the newest skill's body, naming the earlier one", () => {
    const block = buildLoadedSkillsContextBlock([
      turn(loadCall("method-spec", "OLD-BODY-MARKER")),
      turn(loadCall("openapi-specs-layout", "NEW-BODY-MARKER")),
    ]);
    expect(block).toContain("NEW-BODY-MARKER");
    expect(block).not.toContain("OLD-BODY-MARKER");
    expect(block).toContain("`method-spec`");
  });

  // A user skill can be edited between two loads in the same chat, so the
  // later read is the truthful one.
  test("collapses a repeated load to its latest body", () => {
    const block = buildLoadedSkillsContextBlock([
      turn(loadCall("method-spec", "FIRST-MARKER", "done", "user")),
      turn(loadCall("method-spec", "SECOND-MARKER", "done", "user")),
    ])!;
    expect(block).toContain("SECOND-MARKER");
    expect(block).not.toContain("FIRST-MARKER");
    expect(block.split("--- SKILL `method-spec`").length - 1).toBe(1);
    expect(block).not.toContain("Also loaded earlier");
  });

  test("carries the active skill's companion files and no one else's", () => {
    const block = buildLoadedSkillsContextBlock([
      turn(loadCall("method-spec", "OLD")),
      turn(readCall("method-spec", "references/structure.md", "STALE-FILE-MARKER")),
      turn(loadCall("openapi-specs-layout", "NEW")),
      turn(readCall("openapi-specs-layout", "refs/layout.md", "LIVE-FILE-MARKER")),
    ])!;
    expect(block).toContain("LIVE-FILE-MARKER");
    expect(block).toContain("--- SKILL FILE `openapi-specs-layout/refs/layout.md` ---");
    expect(block).not.toContain("STALE-FILE-MARKER");
  });

  test("truncates an oversized body and says how to get the rest", () => {
    const huge = "y".repeat(LOADED_SKILL_CONTEXT_CHARS + 5000);
    const block = buildLoadedSkillsContextBlock([turn(loadCall("huge-skill", huge))])!;
    expect(block.length).toBeLessThan(LOADED_SKILL_CONTEXT_CHARS + 2000);
    expect(block).toContain("…truncated");
    expect(block).toContain("`huge-skill`");
  });
});
