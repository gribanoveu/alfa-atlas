import { createElement } from "react";
import type { Section } from "./types";
import { InlineHtml } from "./InlineHtml";
import { AscBlockList } from "./AscBlockList";

/**
 * Секция AsciiDoc. Уровень (`getLevel()`) соответствует `=`-маркерам:
 * 0 — заголовок документа (document title), 1 — `=`, 2 — `==` и т.д.
 * Рендерится как `<h1>`-`<h6>` + рекурсивно дети.
 */
export function AscSection({ section }: { section: Section }) {
  const level = section.getLevel() ?? 1;
  const title = section.title;
  const tag = `h${Math.min(Math.max(level, 1), 6)}`;

  return (
    <section className="asc-section">
      {title
        ? createElement(tag, null, <InlineHtml html={title} />)
        : null}
      <AscBlockList blocks={section.getBlocks()} />
    </section>
  );
}
