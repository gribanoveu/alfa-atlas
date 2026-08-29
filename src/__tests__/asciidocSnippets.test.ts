import { describe, expect, test } from "bun:test";
import { ASCIIDOC_SNIPPETS } from "../lib/asciidocSnippets";
import { BANG_COMMANDS } from "../hooks/useMonacoCompletions";

/** `${1:default}` → `default`, `${1}`/`$0` → nothing, `\{` → `{`.
 * The default may itself contain escaped braces (`${2:\{host\}/path}`), so the
 * placeholder body is matched as "escaped char, or anything but `\` and `}`". */
function stripTabStops(insertText: string): string {
  return insertText
    .replace(/\$\{\d+:((?:\\.|[^\\}])*)\}/g, "$1")
    .replace(/\$\{\d+\}/g, "")
    .replace(/\$\d+/g, "")
    .replace(/\\([{}$])/g, "$1");
}

function snippet(id: string) {
  const found = ASCIIDOC_SNIPPETS.find((s) => s.id === id);
  if (!found) throw new Error(`no snippet ${id}`);
  return found;
}

function bang(command: string) {
  const found = BANG_COMMANDS.find((c) => c.command === command);
  if (!found) throw new Error(`no bang command ${command}`);
  return found;
}

/** Which `!command` renders which snippet id. */
const PAIRS: [string, string][] = [
  ["table", "simple-table"],
  ["request", "http-method"],
  ["thrift", "thrift-method"],
  ["response", "response-fields"],
  ["validation", "validation-fields"],
  ["errors", "error-codes"],
  ["json", "source-json"],
  ["note", "note"],
  ["tip", "tip"],
  ["warning", "warning"],
  ["important", "important"],
];

describe("каталог элементов AsciiDoc", () => {
  test.each(PAIRS)("!%s вставляет то же, что сниппет %s", (command, id) => {
    expect(stripTabStops(bang(command).insertText)).toBe(snippet(id).template);
  });

  test("таблицы от четырёх колонок не содержат пустых ячеек", () => {
    // K.4.2/K.5.2 считают непройденной таблицу, где пуста хотя бы одна ячейка,
    // поэтому шаблон, из которого начинают документ, обязан быть заполнен.
    for (const { id, template } of ASCIIDOC_SNIPPETS) {
      for (const table of tables(template)) {
        if (table.columns < 4) continue;
        for (const cell of table.cells) {
          expect(cell.trim(), `пустая ячейка в ${id}`).not.toBe("");
        }
      }
    }
  });

  test("разделы постановки названы так, как их ищет проверка стандарта", () => {
    // K.7.1 ищет раздел «Обработка ошибок»; «Коды ошибок» — подпись к таблице
    // внутри него, а не заголовок.
    expect(snippet("error-codes").template).toStartWith("== Обработка ошибок\n");
    expect(snippet("error-codes").template).toContain("*Коды ошибок*");
    // Таблицы параметров живут на третьем уровне каркаса документа.
    for (const id of ["http-method", "thrift-method", "response-fields", "validation-fields"]) {
      expect(snippet(id).template).toStartWith("=== ");
    }
  });

  test("обязательность пишется одним способом — required/optional", () => {
    for (const { id, template } of ASCIIDOC_SNIPPETS) {
      expect(template, `${id}: колонка называется «Обязательность»`).not.toContain(
        "*Обязательный*",
      );
      for (const line of template.split("\n")) {
        const cell = line.replace(/^\|+/, "").trim();
        expect(["да", "нет", "Да", "Нет"], `${id}: ячейка ${cell}`).not.toContain(cell);
      }
    }
  });

  test("таблица кодов ошибок описана в формате постановки", () => {
    const template = snippet("error-codes").template;
    expect(template).toContain("| *Условие* | *Описание* | *Type* | *Code*");
    expect(template).toContain("VALIDATION_ERROR");
    expect(template).not.toContain("ValidationException");
  });

  test("заголовок документа не отбит от атрибутов шапки пустой строкой", () => {
    // Иначе вставленный следом doc-attrs оказался бы вне шапки и :toc: не
    // сработал бы — ровно то, что ловит диагностика detachedHeaderAttributes.
    const header = snippet("doc-title").template + snippet("doc-attrs").template;
    const lines = header.split("\n");
    expect(lines[0]).toStartWith("= ");
    expect(lines[1]).toStartWith(":");
  });
});

/** Грубый разбор блоков `|===`: число колонок по первой строке и все ячейки. */
function tables(source: string): { columns: number; cells: string[] }[] {
  const out: { columns: number; cells: string[] }[] = [];
  const lines = source.split("\n");
  let current: string[] | null = null;
  for (const line of lines) {
    if (line.trim() === "|===") {
      if (current) {
        out.push(parseTable(current));
        current = null;
      } else {
        current = [];
      }
      continue;
    }
    current?.push(line);
  }
  return out;
}

function parseTable(body: string[]): { columns: number; cells: string[] } {
  const columns = body.map((l) => (l.match(/\|/g) ?? []).length).find((n) => n > 0) ?? 0;
  const cells: string[] = [];
  for (const line of body) {
    const trimmed = line.trim();
    if (!trimmed.startsWith("|") && !trimmed.startsWith(".")) continue;
    for (const cell of trimmed.split("|").slice(1)) cells.push(cell);
  }
  return { columns, cells };
}
