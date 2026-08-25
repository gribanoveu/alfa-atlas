import { describe, expect, test } from "bun:test";
import {
  isUnclosedMacroPrefix,
  shouldInsertMacroBrackets,
} from "../lib/asciidocMacroBrackets";

describe("isUnclosedMacroPrefix", () => {
  test("matches include::, image::, and xref: with a file target", () => {
    expect(isUnclosedMacroPrefix("include::request.adoc")).toBe(true);
    expect(isUnclosedMacroPrefix("include::./request.adoc")).toBe(true);
    expect(isUnclosedMacroPrefix("  include::../shared/common.adoc")).toBe(true);
    expect(isUnclosedMacroPrefix("image::diagram.png")).toBe(true);
    expect(isUnclosedMacroPrefix("xref:other.adoc")).toBe(true);
    expect(isUnclosedMacroPrefix("xref:other.adoc#section-id")).toBe(true);
  });

  test("rejects already-closed macros, folders, and incomplete targets", () => {
    expect(isUnclosedMacroPrefix("include::request.adoc[]")).toBe(false);
    expect(isUnclosedMacroPrefix("include::shared/")).toBe(false);
    expect(isUnclosedMacroPrefix("include::")).toBe(false);
    expect(isUnclosedMacroPrefix("xref:other.adoc#")).toBe(false);
    expect(isUnclosedMacroPrefix("See the request")).toBe(false);
    expect(isUnclosedMacroPrefix("image:inline.png")).toBe(false);
  });
});

describe("shouldInsertMacroBrackets", () => {
  test("inserts when the prefix is an unclosed macro", () => {
    expect(shouldInsertMacroBrackets("include::request.adoc", "")).toBe(true);
    expect(shouldInsertMacroBrackets("include::request.adoc", " more text")).toBe(true);
  });

  test("skips when brackets already follow (xref snippet cursor)", () => {
    expect(shouldInsertMacroBrackets("xref:other.adoc", "[]")).toBe(false);
    expect(shouldInsertMacroBrackets("xref:other.adoc", " []")).toBe(false);
  });
});
