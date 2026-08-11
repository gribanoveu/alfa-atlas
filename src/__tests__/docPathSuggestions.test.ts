import { describe, expect, test } from "bun:test";
import {
  buildDocPathSuggestions,
  INCLUDE_DOC_KINDS,
  XREF_DOC_KINDS,
} from "../lib/docPathSuggestions";
import type { Document, DocumentType } from "../lib/workspaceIndex";

function doc(
  relativePath: string,
  docType: DocumentType = "asciiDoc",
): Document {
  const fileName = relativePath.includes("/")
    ? relativePath.slice(relativePath.lastIndexOf("/") + 1)
    : relativePath;
  return {
    id: relativePath,
    absolutePath: `/repo/${relativePath}`,
    relativePath,
    fileName,
    docType,
    modifiedAt: 0,
  };
}

describe("buildDocPathSuggestions", () => {
  const docsRoot = "/repo/docs";
  const repoRoot = "/repo";

  test("partial with trailing slash keeps only that directory prefix", () => {
    const docs = [
      doc("docs/shared/a.adoc"),
      doc("docs/shared/b.adoc"),
      doc("docs/other/c.adoc"),
    ];
    const result = buildDocPathSuggestions({
      docs,
      sourceDocsRelative: "index.adoc",
      docsRoot,
      repoRoot,
      partial: "shared/",
      kinds: INCLUDE_DOC_KINDS,
    });
    expect(result.map((r) => r.insertText).sort()).toEqual([
      "shared/a.adoc",
      "shared/b.adoc",
    ]);
  });

  test("basename partial matches nested file", () => {
    const docs = [
      doc("docs/shared/common.adoc"),
      doc("docs/other/unrelated.adoc"),
    ];
    const result = buildDocPathSuggestions({
      docs,
      sourceDocsRelative: "modules/api/index.adoc",
      docsRoot,
      repoRoot,
      partial: "common",
      kinds: INCLUDE_DOC_KINDS,
    });
    expect(result.map((r) => r.insertText)).toEqual(["../../shared/common.adoc"]);
    expect(result[0]?.label).toBe("common.adoc");
  });

  test("excludes the current document", () => {
    const docs = [doc("docs/index.adoc"), doc("docs/other.adoc")];
    const result = buildDocPathSuggestions({
      docs,
      sourceDocsRelative: "index.adoc",
      docsRoot,
      repoRoot,
      partial: "",
      kinds: INCLUDE_DOC_KINDS,
    });
    expect(result.map((r) => r.insertText)).toEqual(["other.adoc"]);
  });

  test("include kinds drop json and markdown", () => {
    const docs = [
      doc("docs/a.adoc", "asciiDoc"),
      doc("docs/b.json", "json"),
      doc("docs/c.md", "markdown"),
      doc("docs/d.puml", "plantUml"),
    ];
    const result = buildDocPathSuggestions({
      docs,
      sourceDocsRelative: "index.adoc",
      docsRoot,
      repoRoot,
      partial: "",
      kinds: INCLUDE_DOC_KINDS,
    });
    expect(result.map((r) => r.insertText).sort()).toEqual(["a.adoc", "d.puml"]);
  });

  test("xref kinds keep markdown but not plantuml", () => {
    const docs = [
      doc("docs/a.adoc", "asciiDoc"),
      doc("docs/c.md", "markdown"),
      doc("docs/d.puml", "plantUml"),
    ];
    const result = buildDocPathSuggestions({
      docs,
      sourceDocsRelative: "index.adoc",
      docsRoot,
      repoRoot,
      partial: "",
      kinds: XREF_DOC_KINDS,
    });
    expect(result.map((r) => r.insertText).sort()).toEqual(["a.adoc", "c.md"]);
  });

  test("drops documents outside docsRoot", () => {
    const docs = [doc("docs/in.adoc"), doc("readme.adoc")];
    const result = buildDocPathSuggestions({
      docs,
      sourceDocsRelative: "index.adoc",
      docsRoot,
      repoRoot,
      partial: "",
      kinds: INCLUDE_DOC_KINDS,
    });
    expect(result.map((r) => r.insertText)).toEqual(["in.adoc"]);
  });

  test("ranks same-directory paths above deep ../ paths", () => {
    const docs = [
      doc("docs/modules/far/a.adoc"),
      doc("docs/modules/api/near.adoc"),
    ];
    const result = buildDocPathSuggestions({
      docs,
      sourceDocsRelative: "modules/api/index.adoc",
      docsRoot,
      repoRoot,
      partial: "",
      kinds: INCLUDE_DOC_KINDS,
    });
    expect(result[0]?.insertText).toBe("near.adoc");
    expect(result[1]?.insertText).toBe("../far/a.adoc");
  });

  test("filterText equals partial so Monaco keeps pre-filtered items", () => {
    const docs = [doc("docs/shared/a.adoc")];
    const result = buildDocPathSuggestions({
      docs,
      sourceDocsRelative: "index.adoc",
      docsRoot,
      repoRoot,
      partial: "shared/",
      kinds: INCLUDE_DOC_KINDS,
    });
    expect(result[0]?.filterText).toBe("shared/");
  });
});
