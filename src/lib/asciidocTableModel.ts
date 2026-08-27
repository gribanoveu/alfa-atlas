/**
 * AsciiDoc pipe-table discovery, parse (asciidoctor AST → editable model),
 * and serialize back to `|===` source. Pure module — no React/Monaco imports.
 */

import { load } from "asciidoctor";
import type { AbstractBlock } from "asciidoctor";
import { normalizeBarePipeTables } from "./normalizeBarePipeTables";

const TABLE_FENCE_RE = /^\|={3,}[ \t]*$/;
const BLOCK_ATTR_LINE_RE = /^\[[^\]\n]*\][ \t]*$/;

export type TableBlockRange = {
  /** 1-based line of `[cols=...]` (or other table attrs) immediately before `|===`. */
  attrStartLine: number | null;
  /** 1-based line of opening `|===`. */
  openLine: number;
  /** 1-based line of the first non-empty row inside the table. */
  headerLine: number;
  /** 1-based line of closing `|===`. */
  closeLine: number;
};

export type EditableCell = {
  text: string;
  colspan: number;
  rowspan: number;
};

export type TableRowSection = "head" | "body" | "foot";

export type RowLayout = "horizontal" | "vertical";

export type EditableRow = {
  section: TableRowSection;
  cells: EditableCell[];
  /** How this row was authored in the AsciiDoc source. */
  layout: RowLayout;
};

export type EditableTable = {
  /** Full attribute line before the table, e.g. `[cols="1,1"]`. */
  colsAttribute: string | null;
  hasHeader: boolean;
  rows: EditableRow[];
};

export type ParseTableResult =
  | { ok: true; table: EditableTable }
  | { ok: false; reason: string };

type AscCell = {
  getText(): string | null;
  colspan: number | null;
  rowspan: number | null;
};

type AscTableBlock = AbstractBlock & {
  rows: {
    head: AscCell[][];
    body: AscCell[][];
    foot: AscCell[][];
  };
  hasHeaderOption: boolean;
};

/** Find every `|===` … `|===` pipe table block in `content`. */
export function findTableBlocks(content: string): TableBlockRange[] {
  const lines = content.split("\n");
  const blocks: TableBlockRange[] = [];
  const n = lines.length;
  let i = 0;

  while (i < n) {
    if (!TABLE_FENCE_RE.test(lines[i])) {
      i++;
      continue;
    }

    const openLine = i + 1;
    let j = i + 1;
    while (j < n && !TABLE_FENCE_RE.test(lines[j])) j++;
    const closeLine = j < n ? j + 1 : n;

    let headerLine = openLine + 1;
    while (headerLine <= closeLine - 1 && lines[headerLine - 1]?.trim() === "") {
      headerLine++;
    }
    if (headerLine >= closeLine) {
      i = j + 1;
      continue;
    }

    let attrStartLine: number | null = null;
    let k = openLine - 2;
    while (k >= 0 && lines[k].trim() === "") k--;
    if (k >= 0 && BLOCK_ATTR_LINE_RE.test(lines[k])) {
      attrStartLine = k + 1;
    }

    blocks.push({ attrStartLine, openLine, headerLine, closeLine });
    i = j + 1;
  }

  return blocks;
}

/** Return the table block containing `lineNumber`, or `null`. */
export function findTableBlockAtLine(
  content: string,
  lineNumber: number,
): TableBlockRange | null {
  for (const block of findTableBlocks(content)) {
    const start = block.attrStartLine ?? block.openLine;
    if (lineNumber >= start && lineNumber <= block.closeLine) return block;
  }
  return null;
}

/** Extract the full table source fragment (attrs + fences) for replacement. */
export function sliceTableSource(content: string, range: TableBlockRange): string {
  const lines = content.split("\n");
  const start = (range.attrStartLine ?? range.openLine) - 1;
  const end = range.closeLine;
  return lines.slice(start, end).join("\n");
}

/** Replace a table block in `content` with `newSource`. */
export function replaceTableSource(
  content: string,
  range: TableBlockRange,
  newSource: string,
): string {
  const lines = content.split("\n");
  const start = (range.attrStartLine ?? range.openLine) - 1;
  const end = range.closeLine;
  const replacement = newSource.split("\n");
  return [...lines.slice(0, start), ...replacement, ...lines.slice(end)].join("\n");
}

function htmlToAsciidocInline(html: string | null): string {
  if (!html) return "";
  return html
    .replace(/<strong>([\s\S]*?)<\/strong>/gi, "*$1*")
    .replace(/<em>([\s\S]*?)<\/em>/gi, "_$1_")
    .replace(/<code>([\s\S]*?)<\/code>/gi, "`$1`")
    .replace(/<a [^>]*>([\s\S]*?)<\/a>/gi, "$1")
    .replace(/<br\s*\/?>/gi, " ")
    .replace(/<[^>]+>/g, "")
    .trim();
}

function normalizeSpan(value: number | null | undefined): number {
  return value && value > 1 ? value : 1;
}

function convertCells(cells: AscCell[]): EditableCell[] {
  return cells.map((cell) => ({
    text: htmlToAsciidocInline(cell.getText()),
    colspan: normalizeSpan(cell.colspan),
    rowspan: normalizeSpan(cell.rowspan),
  }));
}

function extractColsAttribute(source: string): string | null {
  for (const line of source.split("\n")) {
    const trimmed = line.trim();
    if (TABLE_FENCE_RE.test(trimmed)) break;
    if (BLOCK_ATTR_LINE_RE.test(trimmed)) return trimmed;
  }
  return null;
}

/** Parse `[cols="1,1,1,3"]` into relative weights; `null` when missing or mismatched. */
export function parseColsWeights(
  colsAttribute: string | null,
  columnCount: number,
): number[] | null {
  if (!colsAttribute || columnCount <= 0) return null;
  const match = colsAttribute.match(/cols=["']([^"']+)["']/);
  if (!match) return null;

  const weights = match[1].split(",").map((part) => {
    const token = part.trim().match(/^(\d+)\*?$/);
    return token ? Number.parseInt(token[1], 10) : null;
  });

  if (weights.length !== columnCount || weights.some((w) => w === null || w <= 0)) {
    return null;
  }
  return weights as number[];
}

function distributeEqual(count: number, totalWidth: number): number[] {
  if (count <= 0) return [];
  const minTotal = count * 48;
  const target = Math.max(totalWidth, minTotal);
  const base = Math.floor(target / count);
  let remainder = target - base * count;
  return Array.from({ length: count }, () => {
    const extra = remainder > 0 ? 1 : 0;
    if (remainder > 0) remainder -= 1;
    return base + extra;
  });
}

function distributeWeighted(
  count: number,
  totalWidth: number,
  weights: number[],
): number[] {
  if (count <= 0 || weights.length !== count) return [];
  const minTotal = count * 48;
  const target = Math.max(totalWidth, minTotal);
  const weightSum = weights.reduce((sum, weight) => sum + weight, 0);
  const raw = weights.map((weight) => (weight / weightSum) * target);
  const floored = raw.map((width) => Math.floor(width));
  let remainder = Math.round(target - floored.reduce((sum, width) => sum + width, 0));
  const order = raw
    .map((width, index) => ({ index, fraction: width - Math.floor(width) }))
    .sort((a, b) => b.fraction - a.fraction);
  for (const entry of order) {
    if (remainder <= 0) break;
    floored[entry.index] += 1;
    remainder -= 1;
  }
  return floored.map((width) => Math.max(48, width));
}

/** Distribute pixel widths using `[cols=...]` weights when available. */
export function distributeColumnWidths(
  count: number,
  totalWidth: number,
  colsAttribute?: string | null,
): number[] {
  const weights = parseColsWeights(colsAttribute ?? null, count);
  if (weights) return distributeWeighted(count, totalWidth, weights);
  return distributeEqual(count, totalWidth);
}

function isHorizontalTableLine(line: string): boolean {
  const trimmed = line.trimStart();
  if (/^\d+\+\|/.test(trimmed)) return true;
  if (!trimmed.startsWith("|")) return false;
  if (/\d+\+\|/.test(trimmed)) return true;
  const content = trimmed.slice(1);
  return /\s\|\s/.test(content);
}

/** Detect horizontal vs vertical row layout from the original table source. */
function detectSourceRowLayouts(source: string): RowLayout[] {
  const lines = source.split("\n");
  const bodyLines: string[] = [];
  let inside = false;

  for (const line of lines) {
    const trimmed = line.trim();
    if (TABLE_FENCE_RE.test(trimmed)) {
      if (inside) break;
      inside = true;
      continue;
    }
    if (inside) bodyLines.push(line);
  }

  const layouts: RowLayout[] = [];
  let index = 0;

  while (index < bodyLines.length) {
    while (index < bodyLines.length && bodyLines[index].trim() === "") {
      index++;
    }
    if (index >= bodyLines.length) break;

    const line = bodyLines[index];
    if (isHorizontalTableLine(line)) {
      layouts.push("horizontal");
      index++;
      continue;
    }

    const group: string[] = [];
    while (index < bodyLines.length && bodyLines[index].trim() !== "") {
      const current = bodyLines[index];
      if (group.length > 0 && isHorizontalTableLine(current)) {
        break;
      }
      const currentTrimmed = current.trimStart();
      if (group.length > 0 && /^\d+\+\|/.test(currentTrimmed)) {
        break;
      }
      group.push(current);
      index++;
    }

    layouts.push("vertical");
  }

  return layouts;
}

function effectiveRowLayout(row: EditableRow): RowLayout {
  if (row.cells.some((cell) => normalizeSpan(cell.colspan) > 1)) {
    return "horizontal";
  }
  return row.layout;
}

function attachRowLayouts(rows: EditableRow[], layouts: RowLayout[]): void {
  for (let i = 0; i < rows.length; i++) {
    rows[i].layout = layouts[i] ?? "horizontal";
  }
}

function findTableNode(doc: Awaited<ReturnType<typeof load>>): AscTableBlock | null {
  for (const block of doc.getBlocks()) {
    if (block.getContext() === "table") return block as AscTableBlock;
  }
  return null;
}

/** Parse a table source fragment into an editable model. */
export async function parseAsciidocTable(source: string): Promise<ParseTableResult> {
  const trimmed = source.trim();
  if (!trimmed) {
    return { ok: false, reason: "Пустой фрагмент таблицы" };
  }

  try {
    const doc = await load(normalizeBarePipeTables(trimmed), {
      standalone: false,
      safe: "server",
    });
    const tableNode = findTableNode(doc);
    if (!tableNode) {
      return { ok: false, reason: "Не удалось распознать таблицу AsciiDoc" };
    }

    const rows: EditableRow[] = [];
    for (const row of tableNode.rows.head) {
      rows.push({ section: "head", cells: convertCells(row), layout: "horizontal" });
    }
    for (const row of tableNode.rows.body) {
      rows.push({ section: "body", cells: convertCells(row), layout: "horizontal" });
    }
    for (const row of tableNode.rows.foot) {
      rows.push({ section: "foot", cells: convertCells(row), layout: "horizontal" });
    }

    if (rows.length === 0) {
      return { ok: false, reason: "Таблица не содержит строк" };
    }

    attachRowLayouts(rows, detectSourceRowLayouts(source));

    for (const row of rows) {
      for (const cell of row.cells) {
        if (cell.text.includes("\n")) {
          return {
            ok: false,
            reason: "Ячейки с блочным содержимым не поддерживаются",
          };
        }
      }
    }

    return {
      ok: true,
      table: {
        colsAttribute: extractColsAttribute(source),
        hasHeader: tableNode.hasHeaderOption && tableNode.rows.head.length > 0,
        rows,
      },
    };
  } catch {
    return { ok: false, reason: "Ошибка разбора таблицы AsciiDoc" };
  }
}

function serializeCellText(text: string, rowspan: number): string {
  const trimmed = text.trim();
  if (rowspan > 1) return `.${rowspan}+${trimmed}`;
  return trimmed;
}

/** Serialize one pipe-table row. */
export function serializeRow(cells: EditableCell[]): string {
  if (cells.length === 0) return "|";

  if (cells.length === 1) {
    const cell = cells[0];
    const colspan = normalizeSpan(cell.colspan);
    const rowspan = normalizeSpan(cell.rowspan);
    const text = serializeCellText(cell.text, rowspan);
    if (colspan > 1) {
      return `${colspan}+| ${text}`;
    }
    return `| ${text}`;
  }

  let line = "";
  for (let i = 0; i < cells.length; i++) {
    const cell = cells[i];
    const colspan = normalizeSpan(cell.colspan);
    const rowspan = normalizeSpan(cell.rowspan);
    const text = serializeCellText(cell.text, rowspan);

    if (i === 0) {
      line = `| ${text}`;
      if (colspan > 1) line += ` ${colspan}+|`;
      continue;
    }

    if (colspan > 1) {
      line += ` ${colspan}+| ${text}`;
    } else {
      line += ` | ${text}`;
    }
  }

  return line;
}

function serializeVerticalRow(cells: EditableCell[]): string[] {
  return cells.map((cell) => {
    const text = serializeCellText(cell.text, normalizeSpan(cell.rowspan));
    return `| ${text}`;
  });
}

function shouldBlankLineAfterRow(table: EditableTable, rowIndex: number): boolean {
  if (rowIndex >= table.rows.length - 1) return false;
  const row = table.rows[rowIndex];
  if (table.hasHeader && row.section === "head") return true;
  return row.section === "body";
}

/** Serialize an editable table back to AsciiDoc pipe-table source. */
export function serializeAsciidocTable(table: EditableTable): string {
  const lines: string[] = [];
  if (table.colsAttribute) lines.push(table.colsAttribute);
  lines.push("|===");

  table.rows.forEach((row, rowIndex) => {
    const layout = effectiveRowLayout(row);
    if (layout === "vertical") {
      lines.push(...serializeVerticalRow(row.cells));
    } else {
      lines.push(serializeRow(row.cells));
    }
    if (shouldBlankLineAfterRow(table, rowIndex)) {
      lines.push("");
    }
  });

  lines.push("|===");
  return lines.join("\n");
}

/** Logical column count (sum of colspans in the widest row). */
export function tableColumnCount(table: EditableTable): number {
  let max = 0;
  for (const row of table.rows) {
    const width = row.cells.reduce((sum, cell) => sum + normalizeSpan(cell.colspan), 0);
    max = Math.max(max, width);
  }
  return max;
}

/** Deep-clone an editable table for modal state. */
export function cloneEditableTable(table: EditableTable): EditableTable {
  return {
    colsAttribute: table.colsAttribute,
    hasHeader: table.hasHeader,
    rows: table.rows.map((row) => ({
      section: row.section,
      layout: row.layout,
      cells: row.cells.map((cell) => ({ ...cell })),
    })),
  };
}

/** Compare table structure for round-trip tests (ignores whitespace in cell text). */
export function tablesStructurallyEqual(a: EditableTable, b: EditableTable): boolean {
  if (a.hasHeader !== b.hasHeader) return false;
  if (a.rows.length !== b.rows.length) return false;
  for (let ri = 0; ri < a.rows.length; ri++) {
    const rowA = a.rows[ri];
    const rowB = b.rows[ri];
    if (rowA.section !== rowB.section) return false;
    if (rowA.layout !== rowB.layout) return false;
    if (rowA.cells.length !== rowB.cells.length) return false;
    for (let ci = 0; ci < rowA.cells.length; ci++) {
      const cellA = rowA.cells[ci];
      const cellB = rowB.cells[ci];
      if (
        cellA.text.trim() !== cellB.text.trim() ||
        normalizeSpan(cellA.colspan) !== normalizeSpan(cellB.colspan) ||
        normalizeSpan(cellA.rowspan) !== normalizeSpan(cellB.rowspan)
      ) {
        return false;
      }
    }
  }
  return true;
}
