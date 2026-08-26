import { describe, expect, test } from "bun:test";
import {
  buildActivePlanContextBlock,
  sliceMessagesForPlanExecution,
} from "../lib/assistantConfig";
import type { ChatMessage } from "../lib/chatBlocks";
import type { PlanRecord, PlanTodo } from "../lib/plans";

function userMsg(id: string, content: string, isPlanExecutionStart = false): ChatMessage {
  return isPlanExecutionStart
    ? { id, role: "user", content, isPlanExecutionStart: true }
    : { id, role: "user", content };
}

function assistantMsg(id: string, text: string): ChatMessage {
  return {
    id,
    role: "assistant",
    blocks: [{ type: "text", id: `${id}-t`, content: text }],
    streaming: false,
  };
}

function todo(partial: Partial<PlanTodo> & Pick<PlanTodo, "id" | "content">): PlanTodo {
  return { status: "pending", note: null, ...partial };
}

function record(overrides: Partial<PlanRecord> = {}): PlanRecord {
  return {
    id: "p1",
    name: "Update auth docs",
    overview: "Document the new auth flow.",
    plan: "# Update auth docs\n\n## Цель\nCover the new flow.",
    todos: [
      todo({ id: "setup", content: "Add setup section", status: "inProgress" }),
      todo({ id: "errors", content: "List errors" }),
    ],
    createdAtMs: 0,
    updatedAtMs: 0,
    chatId: null,
    repoRoot: null,
    ...overrides,
  };
}

describe("buildActivePlanContextBlock", () => {
  test("returns null when no plan is active", () => {
    expect(buildActivePlanContextBlock(null)).toBeNull();
    expect(buildActivePlanContextBlock(null, record())).toBeNull();
  });

  test("id-only fallback when the record was not fetched", () => {
    const block = buildActivePlanContextBlock("p1");
    expect(block).toContain("The active work plan id is `p1`");
    expect(block).toContain("readPlan");
    expect(block).not.toContain("Plan body:");
  });

  test("full snapshot includes markdown, checklist ids, and the current step", () => {
    const block = buildActivePlanContextBlock("p1", record());
    expect(block).toContain("Active work plan `p1` — «Update auth docs»");
    expect(block).toContain("Overview: Document the new auth flow.");
    expect(block).toContain("Current step: Add setup section (id: `setup`)");
    expect(block).toContain("● Add setup section (id: `setup`)   ← текущая");
    expect(block).toContain("○ List errors (id: `errors`)");
    expect(block).toContain("Plan body:");
    expect(block).toContain("# Update auth docs");
    expect(block).not.toContain("call `readPlan` with this id");
  });

  test("cancelled todos keep their note so they are not reinvented", () => {
    const block = buildActivePlanContextBlock(
      "p1",
      record({
        todos: [
          todo({ id: "setup", content: "Add setup section", status: "completed" }),
          todo({
            id: "alt",
            content: "Rewrite the whole folder",
            status: "cancelled",
            note: "too broad",
          }),
        ],
      }),
    );
    expect(block).toContain("✗ Rewrite the whole folder (id: `alt`) (too broad)");
    expect(block).toContain("Current step: (none");
  });
});

describe("sliceMessagesForPlanExecution", () => {
  const planning = [
    userMsg("u1", "please plan the auth docs"),
    assistantMsg("a1", "I looked at three approaches and rejected a rewrite."),
    userMsg("u2", "ok, make the plan"),
    assistantMsg("a2", "Plan is ready."),
  ];

  test("start turn drops the entire planning transcript from the wire", () => {
    expect(sliceMessagesForPlanExecution(planning, true)).toEqual([]);
  });

  test("later turns keep the start message and everything after it", () => {
    const start = userMsg("start", "Начни выполнение плана", true);
    const after = assistantMsg("exec1", "Working on setup.");
    const next = userMsg("u3", "continue");
    const sliced = sliceMessagesForPlanExecution([...planning, start, after, next], false);
    expect(sliced.map((m) => m.id)).toEqual(["start", "exec1", "u3"]);
  });

  test("a second start in the same chat takes the later boundary", () => {
    const first = userMsg("s1", "Начни выполнение плана", true);
    const mid = assistantMsg("e1", "partial");
    const second = userMsg("s2", "Начни выполнение плана", true);
    const sliced = sliceMessagesForPlanExecution([...planning, first, mid, second], false);
    expect(sliced.map((m) => m.id)).toEqual(["s2"]);
  });

  test("no start marker leaves ordinary chat history unchanged", () => {
    expect(sliceMessagesForPlanExecution(planning, false)).toBe(planning);
  });
});
