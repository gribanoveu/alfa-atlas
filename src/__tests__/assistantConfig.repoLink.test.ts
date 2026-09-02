import { describe, expect, test } from "bun:test";
import { buildRepositoryLinkContextBlock } from "../lib/assistantConfig";

describe("buildRepositoryLinkContextBlock", () => {
  test("hands the model the template it should substitute into", () => {
    const block = buildRepositoryLinkContextBlock(
      "https://git.example.net/projects/PROJ/repos/repo/browse/{path}",
    );
    expect(block).toContain("https://git.example.net/projects/PROJ/repos/repo/browse/{path}");
    expect(block).toContain("{path}");
  });

  // Saying nothing is what stops the model from guessing a URL: a
  // repository with no web address must produce no link at all.
  test("says nothing when the repository has no web address", () => {
    expect(buildRepositoryLinkContextBlock(null)).toBeNull();
    expect(buildRepositoryLinkContextBlock("")).toBeNull();
  });
});
