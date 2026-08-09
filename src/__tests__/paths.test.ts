import { describe, expect, test } from "bun:test";
import { docsRootRelativeToRepo } from "../lib/paths";

describe("docsRootRelativeToRepo", () => {
  test("nested docs root returns the relative path", () => {
    expect(docsRootRelativeToRepo("/repo", "/repo/src/docs/asciidoc")).toBe(
      "src/docs/asciidoc",
    );
  });

  test("equal roots return null", () => {
    expect(docsRootRelativeToRepo("/repo", "/repo")).toBeNull();
  });

  test("a docs root that isn't nested under the repo root returns null", () => {
    expect(docsRootRelativeToRepo("/repo", "/elsewhere/docs")).toBeNull();
  });

  test("empty input returns null", () => {
    expect(docsRootRelativeToRepo("", "")).toBeNull();
    expect(docsRootRelativeToRepo("/repo", "")).toBeNull();
    expect(docsRootRelativeToRepo("", "/repo/docs")).toBeNull();
  });

  test("backslash-separated (Windows-style) input is normalized", () => {
    expect(docsRootRelativeToRepo("C:\\repo", "C:\\repo\\src\\docs")).toBe("src/docs");
  });
});
