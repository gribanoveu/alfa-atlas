import { describe, expect, test } from "bun:test";
import {
  docsRootRelativeToRepo,
  joinPath,
  parentPath,
  resolveAssetTargetDocsRelative,
} from "../lib/paths";

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

describe("joinPath", () => {
  test("keeps the separator style of the path it is joining onto", () => {
    expect(joinPath("C:\\repos", "clonned-repo")).toBe("C:\\repos\\clonned-repo");
    expect(joinPath("/home/u/projects", "docs")).toBe("/home/u/projects/docs");
    expect(joinPath("\\\\server\\share", "repo")).toBe("\\\\server\\share\\repo");
  });

  test("a trailing separator does not produce a doubled one", () => {
    expect(joinPath("C:\\repos\\", "repo")).toBe("C:\\repos\\repo");
    expect(joinPath("/home/u/", "repo")).toBe("/home/u/repo");
  });

  test("joining onto a bare root keeps the root's slash", () => {
    expect(joinPath("C:\\", "repo")).toBe("C:\\repo");
    expect(joinPath("/", "repo")).toBe("/repo");
  });
});

describe("parentPath", () => {
  test("drops the last segment on either separator", () => {
    expect(parentPath("C:\\repos\\other\\repo")).toBe("C:\\repos\\other");
    expect(parentPath("/home/u/projects/repo")).toBe("/home/u/projects");
  });

  test("stops at the drive or root instead of eating it", () => {
    expect(parentPath("C:\\repos")).toBe("C:\\");
    expect(parentPath("/repo")).toBe("/");
    expect(parentPath("C:\\")).toBe("C:\\");
  });
});
