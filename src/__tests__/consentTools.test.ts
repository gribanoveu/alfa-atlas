import { describe, expect, test } from "bun:test";
import {
  AUTO_APPROVABLE_TOOL_LABELS,
  CONSENT_TOOLS,
  CONSENT_TOOL_LABELS,
  NO_TIMEOUT_TOOLS,
  isAutoApprovable,
} from "../lib/assistantConfig";

/** These sets mirror `ToolName::auto_approvable` on the Rust side, which
 * refuses to persist a grant for a consent tool no matter what the UI sends.
 * A tool missing here would render a "Разрешать всегда" checkbox the backend
 * then rejects, and would be auto-denied on a timer — an outcome the model
 * cannot tell apart from a real refusal. */
describe("consent tools", () => {
  test("requestDependencySources is a consent gate, not a convenience", () => {
    expect(CONSENT_TOOLS.has("requestDependencySources")).toBe(true);
    expect(isAutoApprovable("requestDependencySources")).toBe(false);
    expect(NO_TIMEOUT_TOOLS.has("requestDependencySources")).toBe(true);
  });

  test("every consent tool has a label and none is offered as auto-approvable", () => {
    for (const tool of CONSENT_TOOLS) {
      expect(CONSENT_TOOL_LABELS[tool]).toBeTruthy();
      expect(AUTO_APPROVABLE_TOOL_LABELS[tool]).toBeUndefined();
      expect(isAutoApprovable(tool)).toBe(false);
    }
  });
});
