import { describe, expect, test } from "bun:test";
import { load } from "asciidoctor";
import { normalizeBarePipeTables } from "../lib/normalizeBarePipeTables";

describe("normalizeBarePipeTables", () => {
  test("wraps consecutive pipe rows", () => {
    const input = [
      "| col1 | col2 |",
      "| val1 | val2 |",
    ].join("\n");
    expect(normalizeBarePipeTables(input)).toBe(
      ["|===", "| col1 | col2 |", "| val1 | val2 |", "|==="].join("\n"),
    );
  });

  test("leaves already delimited tables unchanged", () => {
    const input = ["|===", "| a | b |", "| c | d |", "|==="].join("\n");
    expect(normalizeBarePipeTables(input)).toBe(input);
  });

  test("does not wrap pipe rows inside listing blocks", () => {
    const input = [
      "----",
      "| not | a | table |",
      "| still | literal |",
      "----",
    ].join("\n");
    expect(normalizeBarePipeTables(input)).toBe(input);
  });

  test("preserves attribute line before wrapped table", () => {
    const input = [
      '[cols="4"]',
      "| h1 | h2 |",
      "| v1 | v2 |",
    ].join("\n");
    expect(normalizeBarePipeTables(input)).toBe(
      [
        '[cols="4"]',
        "|===",
        "| h1 | h2 |",
        "| v1 | v2 |",
        "|===",
      ].join("\n"),
    );
  });

  test("user table parses as table block after normalization", async () => {
    const raw = [
      "Переходы, инициируемые `/file`:",
      "",
      "| *Ссылки в колбеке* |*Статус документа* |*Статус пакета* |*Метрика* |",
      "|`fileLink` и `signLink` заполнены |`READY_FOR_SEND` |`READY_FOR_SEND` |`READY_FOR_SEND` |",
      "|`archiveLink == null` |`AC_RECEIVE_ERROR` |`UNSIGNED` |`UNSIGNED` |",
    ].join("\n");

    const doc = await load(normalizeBarePipeTables(raw), {
      standalone: false,
      safe: "server",
    });
    const contexts = doc.getBlocks().map((b) => b.getContext());
    expect(contexts).toEqual(["paragraph", "table"]);
  });
});
