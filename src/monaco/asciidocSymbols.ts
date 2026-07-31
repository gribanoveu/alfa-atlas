import type * as Monaco from "monaco-editor";
import type { languages } from "monaco-editor";

/**
 * Builds a nested `DocumentSymbol` tree from raw AsciiDoc text so Monaco's
 * sticky-scroll ("outline model") has something to pin — section headings,
 * table header rows, NOTE/TIP/WARNING blocks, titled listing blocks. Monaco
 * tries outline model → folding-range model → indentation model and stops
 * at the first non-empty result (doesn't merge them), so this single
 * provider has to cover everything we want pinned, not several providers.
 *
 * The regexes here are deliberately kept in sync with the ones in
 * `asciidocLanguage.ts`'s Monarch grammar (headings, table fences,
 * admonition/block-attribute lines) so the outline agrees with what's
 * actually highlighted as a heading/table/etc.
 */

const HEADING_RE = /^(={1,6})(?=\s)\s*(.*)$/;
const TABLE_FENCE_RE = /^\|={3,}[ \t]*$/;
const ADMONITION_BLOCK_ATTR_RE =
  /^\[(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\][ \t]*$/;
// `====`+ (example/admonition block fence) never collides with HEADING_RE:
// a heading requires whitespace + title text right after the `=` run, while
// a bare fence line is just the `=` run and nothing else.
const EXAMPLE_FENCE_RE = /^={4,}[ \t]*$/;
const LISTING_FENCE_RE = /^-{4,}[ \t]*$/;
const LITERAL_FENCE_RE = /^\.{4,}[ \t]*$/;
// AsciiDoc block title (`.Title`) — one dot, then a non-dot/non-space char,
// so it can't be confused with an ordered-list marker (`. text`, needs a
// trailing space right after the dot) or a `....` literal-block fence.
const TITLED_BLOCK_RE = /^\.([^.\s].*)$/;
const BLOCK_ATTR_LINE_RE = /^\[[^\]\n]*\][ \t]*$/;

function isFence(line: string): boolean {
  return (
    LISTING_FENCE_RE.test(line) ||
    LITERAL_FENCE_RE.test(line) ||
    EXAMPLE_FENCE_RE.test(line)
  );
}

type RawBlock =
  | {
      kind: "heading";
      level: number;
      name: string;
      detail: string;
      startLine: number;
      endLine: number;
    }
  | {
      kind: "table" | "admonition" | "titled";
      name: string;
      detail: string;
      startLine: number;
      endLine: number;
    };

/** Single forward pass: finds every heading/table/admonition/titled block. */
function scanBlocks(lines: string[]): RawBlock[] {
  const blocks: RawBlock[] = [];
  const n = lines.length;
  let i = 0;

  while (i < n) {
    const line = lines[i];

    const heading = HEADING_RE.exec(line);
    if (heading) {
      const level = heading[1].length;
      const title = heading[2].trim();
      blocks.push({
        kind: "heading",
        level,
        name: title || "=".repeat(level),
        detail: "",
        startLine: i + 1,
        // Closed properly in `closeHeadingRanges` once every heading is known.
        endLine: n,
      });
      i++;
      continue;
    }

    if (TABLE_FENCE_RE.test(line)) {
      let j = i + 1;
      while (j < n && lines[j].trim() === "") j++;
      const headerLine = j < n ? j : i;
      let close = j + 1;
      while (close < n && !TABLE_FENCE_RE.test(lines[close])) close++;
      const closeLine = close < n ? close : n - 1;
      blocks.push({
        kind: "table",
        name: "Таблица",
        detail: (lines[headerLine] ?? "").trim().slice(0, 80),
        startLine: headerLine + 1,
        endLine: closeLine + 1,
      });
      i = closeLine + 1;
      continue;
    }

    const admonition = ADMONITION_BLOCK_ATTR_RE.exec(line);
    if (admonition && i + 1 < n && EXAMPLE_FENCE_RE.test(lines[i + 1])) {
      let close = i + 2;
      while (close < n && !EXAMPLE_FENCE_RE.test(lines[close])) close++;
      const closeLine = close < n ? close : n - 1;
      blocks.push({
        kind: "admonition",
        name: admonition[1],
        detail: "",
        startLine: i + 1,
        endLine: closeLine + 1,
      });
      i = closeLine + 1;
      continue;
    }

    const titled = TITLED_BLOCK_RE.exec(line);
    if (titled) {
      let k = i + 1;
      if (k < n && BLOCK_ATTR_LINE_RE.test(lines[k])) k++;
      if (k < n && isFence(lines[k])) {
        let close = k + 1;
        while (close < n && !isFence(lines[close])) close++;
        const closeLine = close < n ? close : n - 1;
        blocks.push({
          kind: "titled",
          name: titled[1].trim(),
          detail: "",
          startLine: i + 1,
          endLine: closeLine + 1,
        });
        i = closeLine + 1;
        continue;
      }
    }

    i++;
  }

  return blocks;
}

/** Extends each heading's range to just before the next same-or-shallower heading (or EOF). */
function closeHeadingRanges(blocks: RawBlock[], totalLines: number): void {
  const headings = blocks.filter(
    (b): b is Extract<RawBlock, { kind: "heading" }> => b.kind === "heading",
  );
  for (let idx = 0; idx < headings.length; idx++) {
    const heading = headings[idx];
    let end = totalLines;
    for (let j = idx + 1; j < headings.length; j++) {
      if (headings[j].level <= heading.level) {
        end = headings[j].startLine - 1;
        break;
      }
    }
    heading.endLine = end;
  }
}

type TreeNode = RawBlock & { children: TreeNode[] };

/**
 * Nests every block under the innermost heading open at that point in the
 * document — headings push/pop a stack by level (closing shallower-or-equal
 * sections), non-heading blocks simply attach to whatever heading is
 * currently on top. This is what makes sticky scroll stack a section title
 * above a table header (or admonition, or titled block) at once.
 */
function buildTree(blocks: RawBlock[]): TreeNode[] {
  const sorted = [...blocks].sort((a, b) => a.startLine - b.startLine);
  const root: TreeNode[] = [];
  const stack: { level: number; node: TreeNode }[] = [];

  for (const block of sorted) {
    const node: TreeNode = { ...block, children: [] };
    if (block.kind === "heading") {
      while (stack.length && stack[stack.length - 1].level >= block.level) {
        stack.pop();
      }
      const parent = stack[stack.length - 1]?.node ?? null;
      (parent ? parent.children : root).push(node);
      stack.push({ level: block.level, node });
    } else {
      const parent = stack[stack.length - 1]?.node ?? null;
      (parent ? parent.children : root).push(node);
    }
  }

  return root;
}

function kindFor(
  monaco: typeof Monaco,
  kind: RawBlock["kind"],
): languages.SymbolKind {
  const K = monaco.languages.SymbolKind;
  switch (kind) {
    case "heading":
      return K.String;
    case "table":
      return K.Array;
    case "admonition":
      return K.Event;
    case "titled":
      return K.Function;
  }
}

function toDocumentSymbol(
  monaco: typeof Monaco,
  node: TreeNode,
): languages.DocumentSymbol {
  return {
    name: node.name,
    detail: node.detail,
    kind: kindFor(monaco, node.kind),
    tags: [],
    range: new monaco.Range(node.startLine, 1, node.endLine, 1),
    // Large end column is fine — Monaco clamps it to the line's real length.
    selectionRange: new monaco.Range(node.startLine, 1, node.startLine, 1_000_000),
    children: node.children.map((child) => toDocumentSymbol(monaco, child)),
  };
}

export function buildAsciidocSymbols(
  monaco: typeof Monaco,
  text: string,
): languages.DocumentSymbol[] {
  const lines = text.split(/\r\n|\r|\n/);
  const blocks = scanBlocks(lines);
  closeHeadingRanges(blocks, lines.length);
  const tree = buildTree(blocks);
  return tree.map((node) => toDocumentSymbol(monaco, node));
}
