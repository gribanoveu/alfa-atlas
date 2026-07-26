import type { AbstractBlock, List, Section } from "./types";
import { AscAdmonition } from "./AscAdmonition";
import { AscCodeBlock } from "./AscCodeBlock";
import { AscImage } from "./AscImage";
import { AscList } from "./AscList";
import { AscLiteral } from "./AscLiteral";
import { AscParagraph } from "./AscParagraph";
import { AscQuote } from "./AscQuote";
import { AscSection } from "./AscSection";
import { AscTable } from "./AscTable";
import type { AscTable as AscTableType } from "./types";

type AscBlockListProps = {
  blocks: AbstractBlock[];
  docsRoot?: string | null;
};

/**
 * Рекурсивно обходит список блоков и рендерит каждый подходящим
 * React-компонентом по `getContext()`. Для неизвестных контекстов —
 * fallback: рендерим вложенные блоки (если есть) или исходный текст.
 */
export function AscBlockList({ blocks, docsRoot = null }: AscBlockListProps) {
  return (
    <>
      {blocks.map((block, i) => (
        <AscBlock key={i} block={block} docsRoot={docsRoot} />
      ))}
    </>
  );
}

function AscBlock({
  block,
  docsRoot,
}: {
  block: AbstractBlock;
  docsRoot: string | null;
}) {
  const ctx = block.getContext();

  switch (ctx) {
    case "section":
      return <AscSection section={block as unknown as Section} />;
    case "paragraph":
      return <AscParagraph block={block} />;
    case "ulist":
    case "olist":
      return <AscList list={block as unknown as List} />;
    case "dlist":
      // Description list — минимальная поддержка как <dl>.
      return <AscDescriptionList block={block} />;
    case "table":
      return <AscTable table={block as unknown as AscTableType} />;
    case "admonition":
      return <AscAdmonition block={block} />;
    case "listing":
      return <AscCodeBlock block={block} />;
    case "literal":
      return <AscLiteral block={block} />;
    case "image":
      return <AscImage block={block} docsRoot={docsRoot} />;
    case "quote":
      return <AscQuote block={block} />;
    case "example":
      return (
        <div className="asc-example">
          <AscBlockList blocks={block.getBlocks()} docsRoot={docsRoot} />
        </div>
      );
    case "sidebar":
      return (
        <aside className="asc-sidebar">
          {block.title ? <div className="asc-sidebar-title">{block.title}</div> : null}
          <AscBlockList blocks={block.getBlocks()} docsRoot={docsRoot} />
        </aside>
      );
    case "pass":
      // Raw content pass-through — пропускаем.
      return null;
    case "floating_title":
      return block.title ? (
        <h2 className="asc-floating-title">{block.title}</h2>
      ) : null;
    case "open":
      return <AscBlockList blocks={block.getBlocks()} docsRoot={docsRoot} />;
    default:
      return <AscUnknownBlock block={block} docsRoot={docsRoot} />;
  }
}

function AscDescriptionList({ block }: { block: AbstractBlock }) {
  const items = (block as unknown as {
    getItems: () => Array<[AbstractBlock[], AbstractBlock | null]>;
  }).getItems();
  return (
    <dl className="asc-dlist">
      {items.map(([terms, desc], i) => (
        <div key={i} className="asc-dlist-item">
          {terms.map((t, ti) => (
            <dt key={ti}>{safeGetText(t) ?? ""}</dt>
          ))}
          {desc ? <dd>{safeGetText(desc) ?? ""}</dd> : null}
        </div>
      ))}
    </dl>
  );
}

function safeGetText(block: AbstractBlock): string | null {
  const fn = (block as unknown as { getText?: () => string | null }).getText;
  return typeof fn === "function" ? fn.call(block) : null;
}

function AscUnknownBlock({
  block,
  docsRoot,
}: {
  block: AbstractBlock;
  docsRoot: string | null;
}) {
  const text = safeGetText(block);
  return (
    <div className="asc-unknown" data-context={block.getContext()}>
      {text ? <p>{text}</p> : null}
      <AscBlockList blocks={block.getBlocks()} docsRoot={docsRoot} />
    </div>
  );
}
