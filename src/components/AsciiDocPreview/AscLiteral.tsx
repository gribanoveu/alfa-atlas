import type { AbstractBlock } from "./types";

/**
 * Literal-блок (отступной блок без подстановок, `literal`/пример).
 * asciidoctor предоставляет исходный текст через `getSource()`.
 */
export function AscLiteral({ block }: { block: AbstractBlock }) {
  const source = safeGetSource(block) ?? "";
  const lines = source.split("\n");
  return (
    <pre className="asc-literal">
      <code>
        {lines.map((line, i) => (
          <span key={i} className="asc-literal-line">
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
