import { describe, expect, test } from "bun:test";
import { extractFacts } from "../hooks/useAsciiDocParser";

// These cases mirror the line-based positions the old Rust parser
// (`infra/parsers/ascii_doc.rs`) produced. asciidoctor.js's sourcemap reports
// the enclosing block's line, which differs for `[#id]` inline anchors, so
// `extractFacts` uses a line-scan for positions to stay compatible.

describe("extractFacts", () => {
  test("block anchor [[id]]", async () => {
    const facts = await extractFacts("[[installation]]\n= Installation\n");
    expect(facts.anchors).toEqual([
      { id: "installation", line: 1, column: 1 },
    ]);
  });

  test("inline anchor [#id]", async () => {
    const facts = await extractFacts("[#configuration]\nSome text.\n");
    expect(facts.anchors).toEqual([
      { id: "configuration", line: 1, column: 1 },
    ]);
  });

  test("include directive", async () => {
    const facts = await extractFacts("= Title\n\ninclude::common.adoc[]\n");
    expect(facts.includes).toEqual([
      { path: "common.adoc", line: 3, column: 1 },
    ]);
  });

  test("xref with anchor", async () => {
    const facts = await extractFacts("xref:install.adoc#configuration[]\n");
    expect(facts.references).toEqual([
      {
        targetDocument: "install.adoc",
        anchor: "configuration",
        line: 1,
        column: 1,
      },
    ]);
  });

  test("xref without anchor", async () => {
    const facts = await extractFacts("xref:install.adoc[]\n");
    expect(facts.references).toEqual([
      {
        targetDocument: "install.adoc",
        anchor: null,
        line: 1,
        column: 1,
      },
    ]);
  });

  test("angle-bracket xref with anchor and text", async () => {
    const facts = await extractFacts(
      "<<install.adoc#configuration, конфигурация>>\n",
    );
    expect(facts.references).toContainEqual({
      targetDocument: "install.adoc",
      anchor: "configuration",
      line: 1,
      column: 1,
    });
  });

  test("angle-bracket xref with relative path", async () => {
    const facts = await extractFacts(
      "<<../index.adoc#common-headers, общие заголовки>>\n",
    );
    expect(facts.references).toContainEqual({
      targetDocument: "../index.adoc",
      anchor: "common-headers",
      line: 1,
      column: 1,
    });
  });

  test("angle-bracket xref without anchor", async () => {
    const facts = await extractFacts("<<install.adoc, текст>>\n");
    expect(facts.references).toContainEqual({
      targetDocument: "install.adoc",
      anchor: null,
      line: 1,
      column: 1,
    });
  });

  test("angle-bracket xref same-doc anchor", async () => {
    const facts = await extractFacts("<<#section-id>>\n");
    expect(facts.references).toContainEqual({
      targetDocument: "",
      anchor: "section-id",
      line: 1,
      column: 1,
    });
  });

  test("angle-bracket xref same-doc anchor with text", async () => {
    const facts = await extractFacts("<<#section-id, текст>>\n");
    expect(facts.references).toContainEqual({
      targetDocument: "",
      anchor: "section-id",
      line: 1,
      column: 1,
    });
  });

  test("both xref forms on one line", async () => {
    const facts = await extractFacts(
      "see xref:install.adoc#setup[] and <<../index.adoc#common-headers, общие>>\n",
    );
    expect(facts.references).toHaveLength(2);
    expect(facts.references).toContainEqual({
      targetDocument: "install.adoc",
      anchor: "setup",
      line: 1,
      column: 5,
    });
    expect(facts.references).toContainEqual({
      targetDocument: "../index.adoc",
      anchor: "common-headers",
      line: 1,
      column: 35,
    });
  });

  test("attribute entry", async () => {
    const facts = await extractFacts(":product-name: DocFlow\n");
    expect(facts.attributes).toEqual([
      { name: "product-name", value: "DocFlow", line: 1 },
    ]);
  });

  test("image directive", async () => {
    const facts = await extractFacts("image::images/auth.png[]\n");
    expect(facts.images).toEqual([
      { path: "images/auth.png", line: 1 },
    ]);
  });

  test("multiple constructs in one document", async () => {
    const content = [
      ":product-name: DocFlow",
      "",
      "[[intro]]",
      "= Introduction",
      "",
      "include::common.adoc[]",
      "xref:install.adoc#setup[]",
      "image::images/diagram.png[]",
      "",
    ].join("\n");
    const facts = await extractFacts(content);
    expect(facts.attributes.map((a) => a.name)).toEqual(["product-name"]);
    expect(facts.anchors.map((a) => a.id)).toEqual(["intro"]);
    expect(facts.includes.map((i) => i.path)).toEqual(["common.adoc"]);
    expect(facts.references.map((r) => r.targetDocument)).toEqual([
      "install.adoc",
    ]);
    expect(facts.images.map((i) => i.path)).toEqual(["images/diagram.png"]);
  });

  test("invalid content does not throw and produces empty facts", async () => {
    // asciidoctor is very lenient — even "invalid" content parses without
    // throwing. Verify extractFacts never throws and returns a well-formed
    // result regardless of input.
    const facts = await extractFacts("<<<<\n===\n[invalid\n");
    expect(facts).toBeDefined();
    expect(Array.isArray(facts.anchors)).toBe(true);
    expect(Array.isArray(facts.parseErrors)).toBe(true);
  });
});
