import type { MouseEvent } from "react";

/**
 * Рендерит inline-текст AsciiDoc, в котором asciidoctor уже применил
 * подстановки (bold/italic/mono/links/footnotes и т.д.) и вернул HTML.
 *
 * asciidoctor в `safe: "safe"` режиме генерирует ограниченный набор тегов
 * (strong/em/code/a/span/br/sup/sub), поэтому HTML из `getText()`/`content()`
 * безопасно передать в `dangerouslySetInnerHTML`. Это единственное место
 * в превью, где используется небезопасный HTML-инъект — все блочные
 * структуры рендерятся настоящими React-компонентами.
 *
 * Единственное место, где asciidoctor инжектит `<a>` — поэтому клики по
 * xref-ссылкам (включая угловую форму `<<target#anchor,text>>`) ловим тут
 * одним обработчиком на корневом `<span>`.
 */
export function InlineHtml({
  html,
  onOpenXref,
}: {
  html: string | null | undefined;
  onOpenXref?: (href: string) => void;
}) {
  if (!html) return null;

  const handleClick = (event: MouseEvent<HTMLSpanElement>) => {
    if (!onOpenXref) return;
    const target = event.target as HTMLElement | null;
    const anchor = target?.closest?.("a");
    if (!anchor) return;
    const href = anchor.getAttribute("href");
    if (!href) return;
    event.preventDefault();
    onOpenXref(href);
  };

  return (
    <span dangerouslySetInnerHTML={{ __html: html }} onClick={handleClick} />
  );
}
