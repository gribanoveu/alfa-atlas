import { describe, expect, test } from "bun:test";
import { buildSystemPromptForConversationMode } from "../lib/assistantConfig";
import type { LlmToolDefinition } from "../lib/aiTools";

const def = (name: string): LlmToolDefinition => ({
  name,
  description: `${name} does something`,
  parameters: { type: "object", properties: {} },
});

// With no project open the backend (`current_scope_or_empty`) still hands
// back the project-free tools, so the list is not empty — "no project" is
// derived from `readFile` being absent, not from an empty list.
const NO_PROJECT_TOOLS = [def("skill"), def("visualize"), def("askUser")];
const WITH_PROJECT_TOOLS = [def("readFile"), def("semanticSearch"), def("writeFile")];

describe("access level in the system prompt", () => {
  for (const mode of ["agent", "plan", "question"] as const) {
    test(`${mode} mode reports the no-active-project access level`, () => {
      const prompt = buildSystemPromptForConversationMode(
        mode,
        "docsOnly",
        null,
        NO_PROJECT_TOOLS,
        null,
      );
      expect(prompt).toContain("Access mode: **No active project**");
      // One level, not "Docs-only" plus a separate note about the project.
      expect(prompt).not.toContain("Docs-only");
    });

    test(`${mode} mode reports the persisted mode when files are reachable`, () => {
      const prompt = buildSystemPromptForConversationMode(
        mode,
        "docsOnly",
        null,
        WITH_PROJECT_TOOLS,
        null,
      );
      expect(prompt).toContain("Access mode: **Docs-only**");
      expect(prompt).not.toContain("No active project");
    });
  }
});
