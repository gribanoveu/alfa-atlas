/**
 * Рендерит inline-текст AsciiDoc, в котором asciidoctor уже применил
 * подстановки (bold/italic/mono/links/footnotes и т.д.) и вернул HTML.
 *
 * asciidoctor в `safe: "safe"` режиме генерирует ограниченный набор тегов
 * (strong/em/code/a/span/br/sup/sub), поэтому HTML из `getText()`/`content()`
 * безопасно передать в `dangerouslySetInnerHTML`. Это единственное место
 * в превью, где используется небезопасный HTML-инъект — все блочные
 * структуры рендерятся настоящими React-компонентами.
 */
export function InlineHtml({ html }: { html: string | null | undefined }) {
  if (!html) return null;
  return <span dangerouslySetInnerHTML={{ __html: html }} />;
}
