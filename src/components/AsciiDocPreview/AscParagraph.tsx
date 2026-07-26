import type { AbstractBlock } from "./types";
import { InlineHtml } from "./InlineHtml";
import { AscBlockList } from "./AscBlockList";
import { useBlockContent } from "./useBlockContent";

type XrefHandler = (href: string) => void;

/**
 * Блок-параграф. asciidoctor применяет inline-подстановки (bold/mono/links)
 * асинхронно через `content()` — забираем их через `useBlockContent` и
 * рендерим в `<p>`. Если у параграфа есть вложенные блоки — обходим их тоже.
 */
export function AscParagraph({
  block,
  onOpenXref,
}: {
  block: AbstractBlock;
  onOpenXref?: XrefHandler;
}) {
  const html = useBlockContent(block);
  const blocks = block.getBlocks();
  return (
    <div className="asc-paragraph">
      {html ? (
        <p>
          <InlineHtml html={html} onOpenXref={onOpenXref} />
        </p>
      ) : null}
      {blocks.length > 0 ? (
        <AscBlockList blocks={blocks} onOpenXref={onOpenXref} />
      ) : null}
    </div>
  );
}
