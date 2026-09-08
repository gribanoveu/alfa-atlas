import { describe, expect, test } from "bun:test";
import { buildSystemPromptForConversationMode } from "../lib/assistantConfig";
import type { ConversationMode, LlmToolDefinition } from "../lib/aiTools";

// `readFile` present keeps `resolveAccessLevel` off the no-project prompt,
// which drops most of the sections these tests are about.
const TOOLS: LlmToolDefinition[] = [
  { name: "readFile", description: "reads a file", parameters: { type: "object", properties: {} } },
];

function prompt(mode: ConversationMode): string {
  return buildSystemPromptForConversationMode(mode, "docsOnly", null, TOOLS, null);
}

describe("skills router hint", () => {
  // The observed miss: «распиши алгоритм … в документации он плохо описан»
  // raised no skill search at all, and the section written without the
  // skill had to be rewritten two turns later.
  test.each(["agent", "plan", "question"] as const)("%s mode names the method-doc phrasings", (mode) => {
    const text = prompt(mode);
    expect(text).toContain("распиши алгоритм");
    expect(text).toContain("проверь, что метод описан согласно коду");
    expect(text).toContain("before you start reading the implementation");
  });
});

describe("reporting deviations you chose not to fix", () => {
  // Agent mode is where the failure lives — «поправил и отчитался». Plan
  // and Question make no edits, so the rule has nothing to govern there.
  test("agent mode requires unfixed deviations in the closing report", () => {
    const text = prompt("agent");
    expect(text).toContain("A rule you noticed and chose not to apply is a result");
    expect(text).toContain("belongs in the reply");
  });

  test.each(["plan", "question"] as const)("%s mode does not carry the rule", (mode) => {
    expect(prompt(mode)).not.toContain("A rule you noticed and chose not to apply is a result");
  });
});
