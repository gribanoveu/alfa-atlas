import { describe, expect, test } from "bun:test";
import { ASCIIDOC_SNIPPETS } from "../lib/asciidocSnippets";
import {
  distributeColumnWidths,
  findTableBlockAtLine,
  findTableBlocks,
  insertColsWeightAfter,
  parseAsciidocTable,
  parseColsWeights,
  removeColsWeightAt,
  serializeAsciidocTable,
  sliceTableSource,
  tablesStructurallyEqual,
} from "../lib/asciidocTableModel";

const TABLE_SNIPPETS = ASCIIDOC_SNIPPETS.filter((s) => s.category === "tables");

function extractTableFromTemplate(template: string): string {
  const blocks = findTableBlocks(template);
  if (blocks.length === 0) throw new Error("no table in template");
  return sliceTableSource(template, blocks[0]);
}

describe("findTableBlocks", () => {
  test("finds table with cols attribute", () => {
    const content = [
      "intro",
      '[cols="1,1"]',
      "|===",
      "| A | B",
      "",
      "| 1 | 2",
      "|===",
      "outro",
    ].join("\n");

    const blocks = findTableBlocks(content);
    expect(blocks).toHaveLength(1);
    expect(blocks[0].attrStartLine).toBe(2);
    expect(blocks[0].openLine).toBe(3);
    expect(blocks[0].headerLine).toBe(4);
    expect(blocks[0].closeLine).toBe(7);
  });

  test("findTableBlockAtLine matches inside block", () => {
    const content = ["|===", "| A | B", "|==="].join("\n");
    expect(findTableBlockAtLine(content, 2)).not.toBeNull();
    expect(findTableBlockAtLine(content, 99)).toBeNull();
  });
});

describe("parseAsciidocTable", () => {
  test("parses simple 2x2 table", async () => {
    const source = extractTableFromTemplate(
      TABLE_SNIPPETS.find((s) => s.id === "simple-table")!.template,
    );
    const result = await parseAsciidocTable(source);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.table.hasHeader).toBe(true);
    expect(result.table.rows).toHaveLength(2);
    expect(result.table.rows[0].cells).toHaveLength(2);
    expect(result.table.colsAttribute).toBe('[cols="1,1"]');
  });

  test("parses colspan rows in http-method snippet", async () => {
    const source = extractTableFromTemplate(
      TABLE_SNIPPETS.find((s) => s.id === "http-method")!.template,
    );
    const result = await parseAsciidocTable(source);
    expect(result.ok).toBe(true);
    if (!result.ok) return;

    const methodRow = result.table.rows.find(
      (row) => row.cells[0]?.text === "Метод",
    );
    expect(methodRow).toBeDefined();
    expect(methodRow!.cells[1].colspan).toBe(5);
    expect(methodRow!.cells[1].text).toBe("POST");
  });
});

describe("serializeAsciidocTable round-trip", () => {
  for (const snippet of TABLE_SNIPPETS) {
    test(`round-trip: ${snippet.id}`, async () => {
      const source = extractTableFromTemplate(snippet.template);
      const parsed = await parseAsciidocTable(source);
      expect(parsed.ok).toBe(true);
      if (!parsed.ok) return;

      const reserialized = serializeAsciidocTable(parsed.table);
      const reparsed = await parseAsciidocTable(reserialized);
      expect(reparsed.ok).toBe(true);
      if (!reparsed.ok) return;

      expect(tablesStructurallyEqual(parsed.table, reparsed.table)).toBe(true);
    });
  }

  test("serialize preserves cols attribute", async () => {
    const source = '[cols="1,1,1,1,3,1"]\n|===\n| A | B | C | D | E | F\n|===';
    const parsed = await parseAsciidocTable(source);
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) return;
    const out = serializeAsciidocTable(parsed.table);
    expect(out.startsWith('[cols="1,1,1,1,3,1"]')).toBe(true);
  });

  test("preserves space after opening pipe in horizontal header", async () => {
    const source = '[cols="1,1"]\n|===\n| *Параметр* | *Формат*\n|===';
    const parsed = await parseAsciidocTable(source);
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) return;
    const out = serializeAsciidocTable(parsed.table);
    expect(out).toContain("| *Параметр* | *Формат*");
  });

  test("writes a row span before the pipe, not into the cell text", async () => {
    // `.2+|` перед трубой — единственная форма, которую AsciiDoc считает
    // объединением ячеек по вертикали. Так оформлена таблица валидации в
    // стандарте, и раньше редактор таблиц возвращал её как `| .2+A-userId`,
    // теряя ячейку.
    const source = [
      '[cols="1,2,3"]',
      "|===",
      "| *Параметр* | *Условие* | *Результат*",
      "",
      ".2+|A-userId",
      "|Пусто",
      "|Ошибка 400",
      "|Слишком длинный",
      "|Ошибка 400",
      "|===",
    ].join("\n");
    const parsed = await parseAsciidocTable(source);
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) return;

    const out = serializeAsciidocTable(parsed.table);
    expect(out).toContain(".2+| A-userId");
    expect(out).not.toContain("| .2+A-userId");

    const reparsed = await parseAsciidocTable(out);
    expect(reparsed.ok).toBe(true);
    if (!reparsed.ok) return;
    expect(reparsed.table.rows[1].cells[0].rowspan).toBe(2);
    expect(reparsed.table.rows).toHaveLength(3);
  });

  test("writes a leading cell's column span before its pipe", async () => {
    const source = '[cols="1,1,1"]\n|===\n| A | B | C\n\n2+| Занимает две | C2\n|===';
    const parsed = await parseAsciidocTable(source);
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) return;

    const out = serializeAsciidocTable(parsed.table);
    expect(out).toContain("2+| Занимает две | C2");
    const reparsed = await parseAsciidocTable(out);
    expect(reparsed.ok).toBe(true);
    if (!reparsed.ok) return;
    expect(reparsed.table.rows[1].cells[0].colspan).toBe(2);
  });

  test("preserves vertical body row format for thrift-method", async () => {
    const source = extractTableFromTemplate(
      TABLE_SNIPPETS.find((s) => s.id === "thrift-method")!.template,
    );
    const parsed = await parseAsciidocTable(source);
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) return;

    const userDataRow = parsed.table.rows.find((row) =>
      row.cells.some((cell) => cell.text === "userData"),
    );
    expect(userDataRow?.layout).toBe("vertical");

    const out = serializeAsciidocTable(parsed.table);
    expect(out).toContain("| userData\n| struct\n| required\n| Данные пользователя");
    expect(out).toContain("3+| {host}/");
  });
});

describe("parseColsWeights", () => {
  test("parses cols weights", () => {
    expect(parseColsWeights('[cols="1,1,1,3"]', 4)).toEqual([1, 1, 1, 3]);
  });

  test("distributeColumnWidths respects weights", () => {
    const widths = distributeColumnWidths(4, 400, '[cols="1,1,1,3"]');
    expect(widths.reduce((sum, width) => sum + width, 0)).toBe(400);
    expect(widths[3]).toBeGreaterThan(widths[0]);
  });

  test("removeColsWeightAt drops weight for removed column", () => {
    expect(removeColsWeightAt('[cols="1,1,1,3"]', 3, 4)).toBe('[cols="1,1,1"]');
    expect(removeColsWeightAt('[cols="1,1,1,3"]', 0, 4)).toBe('[cols="1,1,3"]');
  });

  test("insertColsWeightAfter adds weight for new column", () => {
    expect(insertColsWeightAfter('[cols="1,1,1,3"]', 1, 4)).toBe('[cols="1,1,1,1,3"]');
  });
});
