import type { AbstractBlock } from "./types";
import { AscBlockList } from "./AscBlockList";

type XrefHandler = (href: string) => void;

/**
 * Quote-блок (`[quote, author, source]` + `____`). Содержимое — вложенные
 * блоки (обычно параграф). Атрибуция и источник берутся из атрибутов.
 */
export function AscQuote({
  block,
  onOpenXref,
}: {
  block: AbstractBlock;
  onOpenXref?: XrefHandler;
}) {
  const author = block.getAttribute("attribution") as string | null;
  const citetitle = block.getAttribute("citetitle") as string | null;
  return (
    <blockquote className="asc-quote">
      <AscBlockList blocks={block.getBlocks()} onOpenXref={onOpenXref} />
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
