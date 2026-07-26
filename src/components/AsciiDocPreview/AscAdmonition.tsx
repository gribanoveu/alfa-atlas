import type { AbstractBlock } from "./types";
import { InlineHtml } from "./InlineHtml";
import { AscBlockList } from "./AscBlockList";
import { useBlockContent } from "./useBlockContent";

const ADMONITION_KINDS = new Set([
  "note",
  "tip",
  "warning",
  "important",
  "caution",
]);

const ADMONITION_LABELS: Record<string, string> = {
  note: "Note",
  tip: "Tip",
  warning: "Warning",
  important: "Important",
  caution: "Caution",
};

/**
 * Admonition-блок: `[NOTE]`, `[TIP]`, `[WARNING]`, `[IMPORTANT]`, `[CAUTION]`,
 * а также inline-форма `NOTE: текст`. Для inline-формы (без вложенных блоков)
 * `content()` возвращает готовый inline-HTML; блочная форма рендерит детей.
 */
export function AscAdmonition({ block }: { block: AbstractBlock }) {
  const attrs = block.getAttributes() as Record<string, string>;
  const kind = (attrs.name || attrs.style || "").toLowerCase();
  const label = ADMONITION_LABELS[kind] ?? kind;
  const tone = ADMONITION_KINDS.has(kind) ? kind : "note";
  const blocks = block.getBlocks();
  const inlineHtml = useBlockContent(block);

  return (
    <div className={`asc-admonition asc-admonition-${tone}`} data-tone={tone}>
      <div className="asc-admonition-label">{label}</div>
      <div className="asc-admonition-content">
        <AscBlockList blocks={blocks} />
        {blocks.length === 0 && inlineHtml ? (
          <InlineHtml html={inlineHtml} />
        ) : null}
      </div>
    </div>
  );
}
