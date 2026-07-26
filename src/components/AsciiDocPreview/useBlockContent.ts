import { useEffect, useState } from "react";
import type { AbstractBlock } from "./types";

/**
 * Достаёт inline-HTML-контент блока через асинхронный `block.content()`.
 *
 * В asciidoctor.js 4.x `getText()` существует только у list items и table
 * cells; у параграфов и адмонишнов его нет. `content()` же асинхронно
 * применяет inline-подстановки (bold/italic/code/links) и возвращает HTML,
 * который безопасно передаётся в `InlineHtml`.
 *
 * Возвращает `null` до разрешения промиса — компонент рендерится в два
 * прохода (пусто → html), на практике незаметно.
 */
export function useBlockContent(block: AbstractBlock): string | null {
  const [html, setHtml] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const result = block.content();
    Promise.resolve(result)
      .then((v: unknown) => {
        if (cancelled) return;
        // `content()` для параграфов/admonition возвращает string; для таблиц
        // и составных блокков может вернуть string[] | object — игнорируем.
        setHtml(typeof v === "string" ? v : null);
      })
      .catch(() => {
        if (!cancelled) setHtml(null);
      });
    return () => {
      cancelled = true;
    };
  }, [block]);

  return html;
}
