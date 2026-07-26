import type { AbstractBlock } from "./types";
import { InlineHtml } from "./InlineHtml";
import { AscBlockList } from "./AscBlockList";

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
 * а также inline-форма `NOTE: текст`.
 */
export function AscAdmonition({ block }: { block: AbstractBlock }) {
  const attrs = block.getAttributes() as Record<string, string>;
  const kind = (attrs.name || attrs.style || "").toLowerCase();
  const label = ADMONITION_LABELS[kind] ?? kind;
  const tone = ADMONITION_KINDS.has(kind) ? kind : "note";

  return (
    <div className={`asc-admonition asc-admonition-${tone}`} data-tone={tone}>
      <div className="asc-admonition-label">{label}</div>
      <div className="asc-admonition-content">
        <AscBlockList blocks={block.getBlocks()} />
        {block.getBlocks().length === 0 ? (
          <InlineHtml html={safeGetText(block)} />
        ) : null}
      </div>
    </div>
  );
}

function safeGetText(block: AbstractBlock): string | null {
  const fn = (block as unknown as { getText?: () => string | null }).getText;
  return typeof fn === "function" ? fn.call(block) : null;
}
