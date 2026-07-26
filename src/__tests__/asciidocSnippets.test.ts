import { describe, expect, test } from "bun:test";
import {
  ASCIIDOC_SNIPPETS,
  filterSnippets,
} from "../lib/asciidocSnippets";
import { isAsciiDocPath } from "../lib/supportedFiles";

describe("filterSnippets", () => {
  test("returns all snippets for empty query", () => {
    expect(filterSnippets("")).toHaveLength(ASCIIDOC_SNIPPETS.length);
    expect(filterSnippets("   ")).toHaveLength(ASCIIDOC_SNIPPETS.length);
  });

  test("filters by label", () => {
    const result = filterSnippets("параметры");
    expect(result.map((s) => s.id)).toEqual(["request-params"]);
  });

  test("filters by description", () => {
    const result = filterSnippets("listing");
    expect(result.some((s) => s.id === "source-json")).toBe(true);
  });

  test("returns empty when nothing matches", () => {
    expect(filterSnippets("xyz-not-found")).toHaveLength(0);
  });
});

describe("isAsciiDocPath", () => {
  test("accepts .adoc and .asciidoc", () => {
    expect(isAsciiDocPath("foo.adoc")).toBe(true);
    expect(isAsciiDocPath("dir/bar.asciidoc")).toBe(true);
  });

  test("rejects other formats", () => {
    expect(isAsciiDocPath("foo.json")).toBe(false);
    expect(isAsciiDocPath("foo.md")).toBe(false);
    expect(isAsciiDocPath("foo")).toBe(false);
  });
});
