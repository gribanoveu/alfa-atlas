import { describe, expect, test } from "bun:test";
import {
  buildDocPathSuggestions,
  deriveFolderPrefixes,
  documentsToPathEntries,
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

function build(
  docs: Document[],
  partial: string,
  sourceDocsRelative = "index.adoc",
  kinds: readonly DocumentType[] | undefined = INCLUDE_DOC_KINDS,
) {
  return buildDocPathSuggestions({
    entries: documentsToPathEntries(docs),
    sourceDocsRelative,
    docsRoot: "/repo/docs",
    repoRoot: "/repo",
    partial,
    kinds,
  });
}

describe("deriveFolderPrefixes", () => {
  test("empty partial yields top-level folders", () => {
    expect(
      deriveFolderPrefixes(["shared/a.adoc", "shared/b.adoc", "other/c.adoc"], ""),
    ).toEqual(["other/", "shared/"]);
  });

  test("directory partial yields next-level folders only", () => {
    expect(
      deriveFolderPrefixes(
        ["shared/a.adoc", "shared/api/x.adoc", "shared/api/y.adoc"],
        "shared/",
      ),
    ).toEqual(["shared/api/"]);
  });

  test("name prefix filters folder segments", () => {
    expect(
      deriveFolderPrefixes(["shared/a.adoc", "other/c.adoc"], "sh"),
    ).toEqual(["shared/"]);
  });
});

describe("buildDocPathSuggestions", () => {
  test("partial with trailing slash keeps direct files and nested folders", () => {
    const docs = [
      doc("docs/shared/a.adoc"),
      doc("docs/shared/b.adoc"),
      doc("docs/shared/api/x.adoc"),
      doc("docs/other/c.adoc"),
    ];
    const result = build(docs, "shared/");
    expect(result.filter((r) => r.kind === "folder").map((r) => r.insertText)).toEqual([
      "shared/api/",
    ]);
    expect(
      result
        .filter((r) => r.kind === "file")
        .map((r) => r.insertText)
        .sort(),
    ).toEqual(["shared/a.adoc", "shared/b.adoc"]);
  });

  test("empty partial shows top-level files and folders only", () => {
    const docs = [
      doc("docs/root.adoc"),
      doc("docs/shared/a.adoc"),
      doc("docs/other/c.adoc"),
    ];
    const result = build(docs, "");
    expect(result.filter((r) => r.kind === "folder").map((r) => r.insertText).sort()).toEqual([
      "other/",
      "shared/",
    ]);
    expect(result.filter((r) => r.kind === "file").map((r) => r.insertText)).toEqual([
      "root.adoc",
    ]);
  });

  test("basename partial matches nested file", () => {
    const docs = [
      doc("docs/shared/common.adoc"),
      doc("docs/other/unrelated.adoc"),
    ];
    const result = build(docs, "common", "modules/api/index.adoc");
    const files = result.filter((r) => r.kind === "file");
    expect(files.map((r) => r.insertText)).toEqual(["../../shared/common.adoc"]);
    expect(files[0]?.label).toBe("common.adoc");
  });

  test("excludes the current document", () => {
    const docs = [doc("docs/index.adoc"), doc("docs/other.adoc")];
    const result = build(docs, "");
    expect(result.filter((r) => r.kind === "file").map((r) => r.insertText)).toEqual([
      "other.adoc",
    ]);
  });

  test("include kinds drop json and markdown", () => {
    const docs = [
      doc("docs/a.adoc", "asciiDoc"),
      doc("docs/b.json", "json"),
      doc("docs/c.md", "markdown"),
      doc("docs/d.puml", "plantUml"),
    ];
    const result = build(docs, "");
    expect(
      result
        .filter((r) => r.kind === "file")
        .map((r) => r.insertText)
        .sort(),
    ).toEqual(["a.adoc", "d.puml"]);
  });

  test("xref kinds keep markdown but not plantuml", () => {
    const docs = [
      doc("docs/a.adoc", "asciiDoc"),
      doc("docs/c.md", "markdown"),
      doc("docs/d.puml", "plantUml"),
    ];
    const result = build(docs, "", "index.adoc", XREF_DOC_KINDS);
    expect(
      result
        .filter((r) => r.kind === "file")
        .map((r) => r.insertText)
        .sort(),
    ).toEqual(["a.adoc", "c.md"]);
  });

  test("drops documents outside docsRoot", () => {
    const docs = [doc("docs/in.adoc"), doc("readme.adoc")];
    const result = build(docs, "");
    expect(result.filter((r) => r.kind === "file").map((r) => r.insertText)).toEqual([
      "in.adoc",
    ]);
  });

  test("ranks same-directory paths above deep ../ paths when browsing", () => {
    const docs = [
      doc("docs/modules/far/a.adoc"),
      doc("docs/modules/api/near.adoc"),
    ];
    const result = build(docs, "", "modules/api/index.adoc");
    // From modules/api/: near.adoc is top-level relative; far/ is a folder.
    expect(result.filter((r) => r.kind === "file").map((r) => r.insertText)).toEqual([
      "near.adoc",
    ]);
    expect(result.filter((r) => r.kind === "folder").map((r) => r.insertText)).toEqual([
      "../far/",
    ]);
  });

  test("filterText equals partial so Monaco keeps pre-filtered items", () => {
    const docs = [doc("docs/shared/a.adoc")];
    const result = build(docs, "shared/");
    expect(result[0]?.filterText).toBe("shared/");
  });

  test("docs pathSpace lists image assets without repo conversion", () => {
    const result = buildDocPathSuggestions({
      entries: [
        { relativePath: "images/logo.png", fileName: "logo.png" },
        { relativePath: "images/icons/a.png", fileName: "a.png" },
      ],
      sourceDocsRelative: "index.adoc",
      docsRoot: "/repo/docs",
      repoRoot: "/repo",
      partial: "images/",
      pathSpace: "docs",
      excludeSelf: false,
    });
    expect(result.filter((r) => r.kind === "folder").map((r) => r.insertText)).toEqual([
      "images/icons/",
    ]);
    expect(result.filter((r) => r.kind === "file").map((r) => r.insertText)).toEqual([
      "images/logo.png",
    ]);
  });
});
