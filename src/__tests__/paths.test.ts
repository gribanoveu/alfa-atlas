import { describe, expect, test } from "bun:test";
import { docsRootRelativeToRepo, resolveAssetTargetDocsRelative } from "../lib/paths";

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

describe("resolveAssetTargetDocsRelative", () => {
  test("resolves bare filename against the document directory", () => {
    expect(resolveAssetTargetDocsRelative("image.png", "api/doc.adoc")).toBe(
      "api/image.png",
    );
  });

  test("collapses document-relative ../ against the source file", () => {
    expect(
      resolveAssetTargetDocsRelative("../images/logo.png", "api/doc.adoc"),
    ).toBe("images/logo.png");
  });

  test("collapses ./ against the source directory", () => {
    expect(resolveAssetTargetDocsRelative("./shot.png", "api/doc.adoc")).toBe(
      "api/shot.png",
    );
  });

  test("keeps path as-is when source file is unknown", () => {
    expect(resolveAssetTargetDocsRelative("images/logo.png", null)).toBe(
      "images/logo.png",
    );
  });
});
