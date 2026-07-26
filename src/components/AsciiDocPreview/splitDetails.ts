import type { AbstractBlock } from "./types";

export type SplitDetailsGroup = {
  kind: "split-details";
  openSource: string;
  innerBlocks: AbstractBlock[];
};

export type BlockListItem =
  | { kind: "block"; block: AbstractBlock }
  | SplitDetailsGroup;

/** Pass-блок с `<details>` без закрывающего `</details>` в том же фрагменте. */
export function isSplitDetailsOpen(source: string): boolean {
  return /<details\b/i.test(source) && !/<\/details>/i.test(source);
}

/** Pass-блок, который содержит только закрывающий `</details>`. */
export function isSplitDetailsClose(source: string): boolean {
  return /^\s*<\/details>\s*$/i.test(source.trim());
}

export function parseDetailsOpen(source: string): {
  detailsAttrs: string;
  summaryHtml: string | null;
  leadingHtml: string;
} | null {
  const match = source.match(/<details\b([^>]*)>([\s\S]*)$/i);
  if (!match) return null;

  const detailsAttrs = match[1] ?? "";
  const rest = match[2] ?? "";
  const summaryMatch = rest.match(/^\s*(<summary\b[^>]*>[\s\S]*?<\/summary>)/i);
  if (summaryMatch) {
    const summaryTag = summaryMatch[1] ?? "";
    const innerMatch = summaryTag.match(/<summary\b[^>]*>([\s\S]*?)<\/summary>/i);
    return {
      detailsAttrs,
      summaryHtml: innerMatch?.[1] ?? null,
      leadingHtml: summaryTag,
    };
  }

  return { detailsAttrs, summaryHtml: null, leadingHtml: rest.trim() };
}

function safeGetSource(block: AbstractBlock): string | null {
  const fn = (block as unknown as { getSource?: () => string }).getSource;
  return typeof fn === "function" ? fn.call(block) : null;
}

/**
 * asciidoctor разбивает `++++ <details>…++++` … `++++ </details> ++++` на
 * отдельные pass-блоки с AsciiDoc-содержимым между ними. Склеиваем такие
 * последовательности в одну render-единицу.
 */
export function expandSplitDetails(blocks: AbstractBlock[]): BlockListItem[] {
  const result: BlockListItem[] = [];
  let i = 0;

  while (i < blocks.length) {
    const block = blocks[i];
    if (block.getContext() === "pass") {
      const openSource = safeGetSource(block);
      if (openSource && isSplitDetailsOpen(openSource)) {
        const innerBlocks: AbstractBlock[] = [];
        let j = i + 1;
        let closed = false;

        while (j < blocks.length) {
          const next = blocks[j];
          if (next.getContext() === "pass") {
            const closeSource = safeGetSource(next);
            if (closeSource && isSplitDetailsClose(closeSource)) {
              closed = true;
              break;
            }
          }
          innerBlocks.push(next);
          j++;
        }

        if (closed) {
          result.push({
            kind: "split-details",
            openSource,
            innerBlocks,
          });
          i = j + 1;
          continue;
        }
      }
    }

    result.push({ kind: "block", block });
    i++;
  }

  return result;
}
