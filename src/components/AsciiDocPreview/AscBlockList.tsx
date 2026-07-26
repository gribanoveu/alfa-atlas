import type * as Monaco from "monaco-editor";
import type { AbstractBlock, List, Section } from "./types";
import { AscAdmonition } from "./AscAdmonition";
import { AscCodeBlock } from "./AscCodeBlock";
import { AscImage } from "./AscImage";
import { AscList } from "./AscList";
import { AscLiteral } from "./AscLiteral";
import { AscParagraph } from "./AscParagraph";
import { AscPlantuml } from "./AscPlantuml";
import { AscQuote } from "./AscQuote";
import { AscSection } from "./AscSection";
import { AscSplitDetails } from "./AscSplitDetails";
import { AscTable } from "./AscTable";
import type { AscTable as AscTableType } from "./types";
import { expandSplitDetails } from "./splitDetails";

type XrefHandler = (href: string) => void;

type AscBlockListProps = {
  blocks: AbstractBlock[];
  docsRoot?: string | null;
  monaco?: typeof Monaco | null;
  onOpenXref?: XrefHandler;
};

/**
 * Рекурсивно обходит список блоков и рендерит каждый подходящим
 * React-компонентом по `getContext()`. Для неизвестных контекстов —
 * fallback: рендерим вложенные блоки (если есть) или исходный текст.
 */
export function AscBlockList({
  blocks,
  docsRoot = null,
  monaco = null,
  onOpenXref,
}: AscBlockListProps) {
  const items = expandSplitDetails(blocks);

  return (
    <>
      {items.map((item, i) =>
        item.kind === "split-details" ? (
          <AscSplitDetails
            key={i}
            openSource={item.openSource}
            innerBlocks={item.innerBlocks}
            docsRoot={docsRoot}
            monaco={monaco}
            onOpenXref={onOpenXref}
          />
        ) : (
          <AscBlock
            key={i}
            block={item.block}
            docsRoot={docsRoot}
            monaco={monaco}
            onOpenXref={onOpenXref}
          />
        ),
      )}
    </>
  );
}

function AscBlock({
  block,
  docsRoot,
  monaco,
  onOpenXref,
}: {
  block: AbstractBlock;
  docsRoot: string | null;
  monaco: typeof Monaco | null;
  onOpenXref?: XrefHandler;
}) {
  const ctx = block.getContext();

  switch (ctx) {
    case "section":
      return (
        <AscSection
          section={block as unknown as Section}
          docsRoot={docsRoot}
          monaco={monaco}
          onOpenXref={onOpenXref}
        />
      );
    case "paragraph":
      return <AscParagraph block={block} onOpenXref={onOpenXref} />;
    case "ulist":
    case "olist":
      return <AscList list={block as unknown as List} onOpenXref={onOpenXref} />;
    case "dlist":
      // Description list — минимальная поддержка как <dl>.
      return <AscDescriptionList block={block} />;
    case "table":
      return <AscTable table={block as unknown as AscTableType} />;
    case "admonition":
      return <AscAdmonition block={block} onOpenXref={onOpenXref} />;
    case "listing": {
      const style = (block.getAttribute("style") as string | null) ?? null;
      if (style === "plantuml") {
        return <AscPlantuml block={block} docsRoot={docsRoot} />;
      }
      return <AscCodeBlock block={block} monaco={monaco} />;
    }
    case "literal":
      return <AscLiteral block={block} />;
    case "image":
      return <AscImage block={block} docsRoot={docsRoot} />;
    case "quote":
      return <AscQuote block={block} onOpenXref={onOpenXref} />;
    case "example":
      return (
        <div className="asc-example">
          <AscBlockList
            blocks={block.getBlocks()}
            docsRoot={docsRoot}
            monaco={monaco}
            onOpenXref={onOpenXref}
          />
        </div>
      );
    case "sidebar":
      return (
        <aside className="asc-sidebar">
          {block.title ? <div className="asc-sidebar-title">{block.title}</div> : null}
          <AscBlockList
            blocks={block.getBlocks()}
            docsRoot={docsRoot}
            monaco={monaco}
            onOpenXref={onOpenXref}
          />
        </aside>
      );
    case "pass":
      return <AscPass block={block} />;
    case "floating_title":
      return block.title ? (
        <h2 className="asc-floating-title">{block.title}</h2>
      ) : null;
    case "open":
      return (
        <AscBlockList
          blocks={block.getBlocks()}
          docsRoot={docsRoot}
          monaco={monaco}
          onOpenXref={onOpenXref}
        />
      );
    default:
      return (
        <AscUnknownBlock
          block={block}
          docsRoot={docsRoot}
          monaco={monaco}
          onOpenXref={onOpenXref}
        />
      );
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
  monaco,
  onOpenXref,
}: {
  block: AbstractBlock;
  docsRoot: string | null;
  monaco: typeof Monaco | null;
  onOpenXref?: XrefHandler;
}) {
  const text = safeGetText(block);
  return (
    <div className="asc-unknown" data-context={block.getContext()}>
      {text ? <p>{text}</p> : null}
      <AscBlockList
        blocks={block.getBlocks()}
        docsRoot={docsRoot}
        monaco={monaco}
        onOpenXref={onOpenXref}
      />
    </div>
  );
}

/**
 * Pass-блок (`++++ … ++++`) — raw passthrough: HTML/SVG и т.п.
 * Разорванные `<details>` обрабатываются в `expandSplitDetails` → `AscSplitDetails`.
 */
function AscPass({ block }: { block: AbstractBlock }) {
  const html = safeGetSource(block);
  if (!html) return null;
  return (
    <div
      className="asc-pass"
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function safeGetSource(block: AbstractBlock): string | null {
  const fn = (block as unknown as { getSource?: () => string }).getSource;
  return typeof fn === "function" ? fn.call(block) : null;
}
