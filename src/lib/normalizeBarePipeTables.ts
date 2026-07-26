/**
 * AsciiDoc pipe tables require `|===` … `|===` delimiters. Many documents
 * omit them; asciidoctor then treats each row as a plain paragraph. This
 * preprocessor wraps consecutive pipe rows in delimiters before `load()`.
 */

const TABLE_DELIMITER = /^\|={3,}\s*$/;
const BLOCK_DELIMITER = /^(?:-{4,}|\.{4,}|={4,}|\*{4,}|\+{4,}|,{4,})\s*$/;

/** Row starts with `|` and has at least one more pipe (cell separator). */
function isBarePipeTableRow(line: string): boolean {
  const trimmed = line.trimStart();
  if (!trimmed.startsWith("|")) return false;
  if (TABLE_DELIMITER.test(trimmed)) return false;
  return (trimmed.match(/\|/g)?.length ?? 0) >= 2;
}

/**
 * Wrap bare pipe-table rows in `|===` delimiters. Leaves delimited tables,
 * listing/literal blocks, and single pipe lines unchanged.
 */
export function normalizeBarePipeTables(content: string): string {
  const lines = content.split("\n");
  const out: string[] = [];
  let inDelimitedBlock = false;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    if (BLOCK_DELIMITER.test(line.trim())) {
      inDelimitedBlock = !inDelimitedBlock;
      out.push(line);
      continue;
    }

    if (inDelimitedBlock) {
      out.push(line);
      continue;
    }

    if (TABLE_DELIMITER.test(line.trim())) {
      out.push(line);
      i++;
      while (i < lines.length && !TABLE_DELIMITER.test(lines[i].trim())) {
        out.push(lines[i]);
        i++;
      }
      if (i < lines.length) {
        out.push(lines[i]);
      }
      continue;
    }

    if (!isBarePipeTableRow(line)) {
      out.push(line);
      continue;
    }

    const tableLines: string[] = [line];
    i++;
    while (i < lines.length && isBarePipeTableRow(lines[i])) {
      tableLines.push(lines[i]);
      i++;
    }
    i--;

    if (tableLines.length >= 2) {
      out.push("|===", ...tableLines, "|===");
    } else {
      out.push(...tableLines);
    }
  }

  return out.join("\n");
}
