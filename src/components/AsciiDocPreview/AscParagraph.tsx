import type { AbstractBlock } from "./types";
import { InlineHtml } from "./InlineHtml";
import { AscBlockList } from "./AscBlockList";
import { useBlockContent } from "./useBlockContent";

/**
 * Блок-параграф. asciidoctor применяет inline-подстановки (bold/mono/links)
 * асинхронно через `content()` — забираем их через `useBlockContent` и
 * рендерим в `<p>`. Если у параграфа есть вложенные блоки — обходим их тоже.
 */
export function AscParagraph({ block }: { block: AbstractBlock }) {
  const html = useBlockContent(block);
  const blocks = block.getBlocks();
  return (
    <div className="asc-paragraph">
      {html ? (
        <p>
          <InlineHtml html={html} />
        </p>
      ) : null}
      {blocks.length > 0 ? <AscBlockList blocks={blocks} /> : null}
    </div>
  );
}
