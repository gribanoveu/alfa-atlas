import type { AbstractBlock } from "./types";
import { InlineHtml } from "./InlineHtml";
import { AscBlockList } from "./AscBlockList";

/**
 * Блок-параграф. Текст с inline-подстановками asciidoctor уже вернул HTML
 * через `getText()`, рендерим его в <p>. Если у параграфа есть вложенные
 * блоки (например, `role` с вложенным контентом) — обходим их тоже.
 */
export function AscParagraph({ block }: { block: AbstractBlock }) {
  const text = safeGetText(block);
  const blocks = block.getBlocks();
  return (
    <div className="asc-paragraph">
      {text !== null ? (
        <p>
          <InlineHtml html={text} />
        </p>
      ) : null}
      {blocks.length > 0 ? <AscBlockList blocks={blocks} /> : null}
    </div>
  );
}

function safeGetText(block: AbstractBlock): string | null {
  // Block имеет `getText()` только для параграфов/листинг-подобных узлов;
  // для составных блоков возвращаем null, чтобы не падать.
  const fn = (block as unknown as { getText?: () => string | null }).getText;
  return typeof fn === "function" ? fn.call(block) : null;
}
