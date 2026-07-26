import { createElement } from "react";
import type * as Monaco from "monaco-editor";
import type { Section } from "./types";
import { InlineHtml } from "./InlineHtml";
import { AscBlockList } from "./AscBlockList";

type XrefHandler = (href: string) => void;

/**
 * Секция AsciiDoc. Уровень (`getLevel()`) соответствует `=`-маркерам:
 * 0 — заголовок документа (document title), 1 — `=`, 2 — `==` и т.д.
 * Рендерится как `<h1>`-`<h6>` + рекурсивно дети.
 *
 * `docsRoot` и `monaco` пробрасываются в дочерний `AscBlockList`, чтобы
 * вложенные блоки (image, plantuml, code) могли резолвить пути и подсвечивать
 * код на любой глубине секции.
 */
export function AscSection({
  section,
  docsRoot = null,
  monaco = null,
  onOpenXref,
}: {
  section: Section;
  docsRoot?: string | null;
  monaco?: typeof Monaco | null;
  onOpenXref?: XrefHandler;
}) {
  const level = section.getLevel() ?? 1;
  const title = section.title;
  const tag = `h${Math.min(Math.max(level, 1), 6)}`;

  return (
    <section className="asc-section">
      {title
        ? createElement(tag, null, <InlineHtml html={title} onOpenXref={onOpenXref} />)
        : null}
      <AscBlockList
        blocks={section.getBlocks()}
        docsRoot={docsRoot}
        monaco={monaco}
        onOpenXref={onOpenXref}
      />
    </section>
  );
}
