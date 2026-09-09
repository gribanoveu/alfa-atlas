import { describe, expect, test } from "bun:test";
import { extractFacts } from "../hooks/useAsciiDocParser";

/** `extractFacts` is what the Rust coordinator indexes the workspace from:
 * every xref it misses is a broken link nobody is warned about, and every
 * line number it gets wrong sends "go to definition" to the wrong place.
 * The scans are exported precisely so this can be checked without the IPC
 * round trip the hook wraps them in. */
describe("extractFacts — anchors", () => {
  test("finds both block and inline anchors with 1-based positions", async () => {
    const facts = await extractFacts("= Doc\n\n[[intro]]\n== Введение\n\n[#detail]\n== Детали\n");

    expect(facts.anchors).toEqual([
      { id: "intro", line: 3, column: 1 },
      { id: "detail", line: 6, column: 1 },
    ]);
  });

  test("an anchor with reftext contributes only its id", async () => {
    const facts = await extractFacts("[[intro,Введение]]\n== Раздел\n");
    expect(facts.anchors.map((a) => a.id)).toEqual(["intro"]);
  });
});

describe("extractFacts — xrefs", () => {
  test("an xref carrying link text is still found", async () => {
    // The ordinary form in real documentation. It used to be invisible here,
    // so neither its target nor its position was ever checked.
    const facts = await extractFacts("См. xref:setup.adoc[Настройка].\n");

    expect(facts.references).toHaveLength(1);
    expect(facts.references[0]).toMatchObject({
      targetDocument: "setup.adoc",
      anchor: null,
      line: 1,
    });
  });

  test("a fragment is split off the target document", async () => {
    const facts = await extractFacts("xref:guide.adoc#install[Установка]\n");
    expect(facts.references[0]).toMatchObject({
      targetDocument: "guide.adoc",
      anchor: "install",
    });
  });

  test("a reference inside a listing block is literal text, not a link", async () => {
    const facts = await extractFacts(
      ["Текст.", "", "----", "xref:nowhere.adoc[Не ссылка]", "----", ""].join("\n"),
    );
    expect(facts.references).toEqual([]);
  });
});

describe("extractFacts — includes", () => {
  test("reports the include directive's own line, not the line after it", async () => {
    // `reader.lineno` points at the next line to read, so this is the
    // off-by-one the include processor exists to correct.
    const facts = await extractFacts("= Doc\n\ninclude::part.adoc[]\n");

    expect(facts.includes).toEqual([{ path: "part.adoc", line: 3, column: 1 }]);
  });

  test("includes are captured without being resolved from disk", async () => {
    // Nothing on disk is called `missing.adoc`; extraction must still succeed.
    const facts = await extractFacts("include::missing.adoc[]\n");

    expect(facts.includes.map((i) => i.path)).toEqual(["missing.adoc"]);
    expect(facts.parseErrors.filter((e) => e.severity === "error")).toEqual([]);
  });
});

describe("extractFacts — diagnostics", () => {
  test("a malformed table is a warning, not a failure", async () => {
    // asciidoctor logs table-layout quirks at ERROR even though the document
    // loads fine; surfacing them as errors would flip the whole index to
    // "failed" over a cosmetic issue.
    const facts = await extractFacts("|===\n| a | b\n| c\n|===\n");

    expect(facts.parseErrors.every((e) => e.severity === "warning")).toBe(true);
  });

  test("a clean document reports nothing", async () => {
    const facts = await extractFacts("= Заголовок\n\nОбычный абзац.\n");
    expect(facts.parseErrors).toEqual([]);
  });
});
