import type { AbstractBlock } from "./types";
import { AscBlockList } from "./AscBlockList";

/**
 * Quote-блок (`[quote, author, source]` + `____`). Содержимое — вложенные
 * блоки (обычно параграф). Атрибуция и источник берутся из атрибутов.
 */
export function AscQuote({ block }: { block: AbstractBlock }) {
  const author = block.getAttribute("attribution") as string | null;
  const citetitle = block.getAttribute("citetitle") as string | null;
  return (
    <blockquote className="asc-quote">
      <AscBlockList blocks={block.getBlocks()} />
      {author || citetitle ? (
        <footer className="asc-quote-attribution">
          {author ? <span className="asc-quote-author">{author}</span> : null}
          {citetitle ? (
            <cite className="asc-quote-cite">{citetitle}</cite>
          ) : null}
        </footer>
      ) : null}
    </blockquote>
  );
}
