import type { AbstractBlock } from "./types";

/**
 * Блок кода / listing / source. asciidoctor предоставляет исходный текст
 * через `getSource()` (моноширинный). Подсветка синтаксиса не реализована —
 * только моноширинный <pre>.
 */
export function AscCodeBlock({ block }: { block: AbstractBlock }) {
  const source = safeGetSource(block) ?? "";
  const lang = block.getAttribute("language") as string | null;
  const lines = source.split("\n");

  return (
    <pre className="asc-code" data-lang={lang ?? undefined}>
      <code>
        {lines.map((line, i) => (
          <span key={i} className="asc-code-line">
            {line || "\u00a0"}
          </span>
        ))}
      </code>
    </pre>
  );
}

function safeGetSource(block: AbstractBlock): string | null {
  const fn = (block as unknown as { getSource?: () => string }).getSource;
  return typeof fn === "function" ? fn.call(block) : null;
}
