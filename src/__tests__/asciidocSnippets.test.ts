import { describe, expect, test } from "bun:test";
import {
  ASCIIDOC_SNIPPETS,
  filterSnippets,
  type AsciiDocSnippet,
} from "../lib/asciidocSnippets";
import { isAsciiDocPath } from "../lib/supportedFiles";

/** A fixed catalogue, so these tests describe how filtering behaves rather
 * than what the shipped snippet list currently happens to contain — that is
 * what `filterSnippets`' second parameter is for. Asserting against the real
 * list is why "filters by label" broke: it pinned a single id, and later
 * snippets both removed that id and added three more matching the query. */
const FIXTURE: AsciiDocSnippet[] = [
  {
    id: "params",
    label: "Параметры запроса",
    category: "tables",
    template: "",
  },
  {
    id: "params-job",
    label: "Параметры запроса для Job",
    category: "tables",
    template: "",
  },
  {
    id: "pipe-table",
    label: "Таблица",
    category: "tables",
    description: "Простая pipe-таблица",
    template: "",
  },
  {
    // No description at all — the one that would throw if the haystack
    // were built without the `?? ""` fallback.
    id: "rule",
    label: "Разделитель",
    category: "structure",
    template: "",
  },
];

describe("filterSnippets", () => {
  test("an empty query returns everything", () => {
    expect(filterSnippets("", FIXTURE)).toEqual(FIXTURE);
    expect(filterSnippets("   ", FIXTURE)).toEqual(FIXTURE);
  });

  test("it matches on the label", () => {
    expect(filterSnippets("разделитель", FIXTURE).map((s) => s.id)).toEqual(["rule"]);
  });

  test("it returns every match, not just the first", () => {
    expect(filterSnippets("параметры", FIXTURE).map((s) => s.id)).toEqual([
      "params",
      "params-job",
    ]);
  });

  test("it matches on the description too", () => {
    expect(filterSnippets("pipe", FIXTURE).map((s) => s.id)).toEqual(["pipe-table"]);
  });

  test("matching is case-insensitive both ways", () => {
    expect(filterSnippets("ПАРАМЕТРЫ", FIXTURE).map((s) => s.id)).toEqual([
      "params",
      "params-job",
    ]);
    expect(filterSnippets("таблица", FIXTURE).map((s) => s.id)).toEqual(["pipe-table"]);
  });

  test("surrounding whitespace is ignored", () => {
    expect(filterSnippets("  разделитель  ", FIXTURE).map((s) => s.id)).toEqual(["rule"]);
  });

  test("a query matching nothing returns nothing", () => {
    expect(filterSnippets("xyz-not-found", FIXTURE)).toHaveLength(0);
  });

  test("it defaults to the shipped catalogue", () => {
    // The only assertion that touches the real list, and it deliberately
    // says nothing about its contents.
    expect(filterSnippets("")).toEqual(ASCIIDOC_SNIPPETS);
    expect(ASCIIDOC_SNIPPETS.length).toBeGreaterThan(0);
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
