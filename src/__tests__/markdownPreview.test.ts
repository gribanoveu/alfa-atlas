import { describe, expect, test } from "bun:test";
import { extensionOf, isMarkdownPath } from "../lib/fileExtensions";

describe("fileExtensions", () => {
  test("extensionOf returns lowercase extension", () => {
    expect(extensionOf("docs/readme.md")).toBe(".md");
    expect(extensionOf("README.MD")).toBe(".md");
    expect(extensionOf("noext")).toBe("");
  });

  test("isMarkdownPath detects markdown files", () => {
    expect(isMarkdownPath("guide.md")).toBe(true);
    expect(isMarkdownPath("guide.markdown")).toBe(true);
    expect(isMarkdownPath("nested/readme.MD")).toBe(true);
    expect(isMarkdownPath("readme.adoc")).toBe(false);
    expect(isMarkdownPath("diagram.mmd")).toBe(false);
  });
});
