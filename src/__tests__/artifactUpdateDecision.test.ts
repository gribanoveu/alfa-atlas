import { describe, expect, test } from "bun:test";
import { decideArtifactUpdate, type ArtifactRecord } from "../lib/artifacts";

function record(overrides: Partial<ArtifactRecord> = {}): ArtifactRecord {
  return {
    id: "a1",
    kind: "jiraTicket",
    title: "Задача",
    purpose: null,
    status: "draft",
    content: {
      kind: "jiraTicket",
      why: "",
      outcome: "",
      solution: "",
      inScope: [],
      outOfScope: [],
      acceptanceCriteria: [],
      definitionOfDone: [],
      risks: [],
      links: [],
      issueKey: "",
    } as ArtifactRecord["content"],
    createdAtMs: 1000,
    updatedAtMs: 2000,
    chatId: null,
    repoRoot: null,
    ...overrides,
  };
}

describe("decideArtifactUpdate", () => {
  test("adopts a newer version when the tab has no unsaved edits", () => {
    expect(
      decideArtifactUpdate(record({ updatedAtMs: 3000 }), record(), false),
    ).toBe("adopt");
  });

  test("holds a newer version back when the user has unsaved edits", () => {
    expect(
      decideArtifactUpdate(record({ updatedAtMs: 3000 }), record(), true),
    ).toBe("hold");
  });

  test("ignores a record the tab is not showing", () => {
    expect(
      decideArtifactUpdate(record({ id: "other", updatedAtMs: 3000 }), record(), false),
    ).toBe("ignore");
  });

  test("ignores the same version — `artifact read` returns one on every read", () => {
    expect(decideArtifactUpdate(record(), record(), false)).toBe("ignore");
    expect(decideArtifactUpdate(record(), record(), true)).toBe("ignore");
  });

  test("ignores an older version — a late result must not undo a newer save", () => {
    expect(
      decideArtifactUpdate(record({ updatedAtMs: 1500 }), record(), false),
    ).toBe("ignore");
  });

  test("adopts when the tab is still loading and has nothing to compare against", () => {
    expect(decideArtifactUpdate(record(), null, false)).toBe("adopt");
  });
});
