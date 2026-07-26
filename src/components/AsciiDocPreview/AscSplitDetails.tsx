import type * as Monaco from "monaco-editor";
import type { AbstractBlock } from "./types";
import { AscBlockList } from "./AscBlockList";
import { InlineHtml } from "./InlineHtml";
import { parseDetailsOpen } from "./splitDetails";

type XrefHandler = (href: string) => void;

type AscSplitDetailsProps = {
  openSource: string;
  innerBlocks: AbstractBlock[];
  docsRoot?: string | null;
  monaco?: typeof Monaco | null;
  onOpenXref?: XrefHandler;
};

/**
 * `<details>` из pass-блоков, разорванных asciidoctor: открывающий pass,
 * AsciiDoc-блоки внутри, закрывающий pass (не рендерится отдельно).
 */
export function AscSplitDetails({
  openSource,
  innerBlocks,
  docsRoot = null,
  monaco = null,
  onOpenXref,
}: AscSplitDetailsProps) {
  const parsed = parseDetailsOpen(openSource);
  if (!parsed) {
    return (
      <div
        className="asc-pass"
        dangerouslySetInnerHTML={{ __html: openSource }}
      />
    );
  }

  const { detailsAttrs, summaryHtml, leadingHtml } = parsed;

  return (
    <details
      className="asc-details"
      {...detailsAttributesToProps(detailsAttrs)}
    >
      {summaryHtml !== null ? (
        <summary>
          <InlineHtml html={summaryHtml} onOpenXref={onOpenXref} />
        </summary>
      ) : leadingHtml ? (
        <summary dangerouslySetInnerHTML={{ __html: leadingHtml }} />
      ) : null}
      <div className="asc-details-body">
        <AscBlockList
          blocks={innerBlocks}
          docsRoot={docsRoot}
          monaco={monaco}
          onOpenXref={onOpenXref}
        />
      </div>
    </details>
  );
}

function detailsAttributesToProps(
  attrs: string,
): Record<string, string | boolean> {
  const props: Record<string, string | boolean> = {};
  const attrRe = /(\w[\w:-]*)(?:="([^"]*)"|='([^']*)'|=([^\s"'>/]+))?/g;
  let match: RegExpExecArray | null;

  while ((match = attrRe.exec(attrs)) !== null) {
    const name = match[1]?.toLowerCase();
    if (!name || name === "class") continue;
    const value = match[2] ?? match[3] ?? match[4] ?? "";
    if (name === "open") {
      props.open = true;
    } else {
      props[name] = value;
    }
  }

  return props;
}
