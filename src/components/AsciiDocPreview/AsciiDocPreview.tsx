import { useCallback, useRef, useState } from "react";
import type * as Monaco from "monaco-editor";
import { extensionOf } from "../../lib/fileExtensions";
import { useAsciiDocRender } from "../../hooks/useAsciiDocRender";
import { AscBlockList } from "./AscBlockList";
import { AscMermaid } from "./AscMermaid";
import { AscPlantuml } from "./AscPlantuml";
import { AscPreviewProvider } from "./AscPreviewContext";
import { AscToc, type AscTocPlacement } from "./AscToc";
import { InlineHtml } from "./InlineHtml";
import { makeDiagramBlock } from "./syntheticBlock";
import type { Section } from "./types";
import "./AsciiDocPreview.css";

type XrefHandler = (href: string) => void;

type AsciiDocPreviewProps = {
  content: string;
  /** Docs-relative path of the file being previewed (e.g. `db.adoc`, `seq.puml`). */
  filePath: string | null;
  docsRoot: string | null;
  /** Monaco namespace — нужен для подсветки кода в AscCodeBlock. */
  monaco: typeof Monaco | null;
  /** Клик по xref-ссылке в превью (path#anchor или #anchor). */
  onOpenXref?: XrefHandler;
};

/** Extensions that are standalone PlantUML sources, not AsciiDoc. */
const PLANTUML_EXTS = new Set([".puml", ".plantuml"]);

/** Extensions that are standalone Mermaid sources, not AsciiDoc. */
const MERMAID_EXTS = new Set([".mmd", ".mermaid"]);

/**
 * Below this share of the window width, the sidebar TOC (`:toc: left/right`)
 * collapses in favor of reading room — covers both the 2-panel split view
 * (preview pane ~half the window) and a full-width preview squeezed by a
 * narrow app window or open sidebars.
 */
const TOC_SIDEBAR_MIN_WIDTH_RATIO = 0.6;

/**
 * Контейнер превью AsciiDoc: парсит контент в AST и рендерит дерево блоков
 * React-компонентами проекта. Состояния: загрузка, ошибка парсинга, пусто,
 * готово.
 *
 * Для `.puml`/`.plantuml` файлов AsciiDoc-парсинг не имеет смысла (контент —
 * чистый PlantUML), поэтому такой файл рендерится одной `AscPlantuml`.
 */
export function AsciiDocPreview({
  content,
  filePath,
  docsRoot,
  monaco,
  onOpenXref,
}: AsciiDocPreviewProps) {
  const ext = filePath ? extensionOf(filePath) : "";
  const isPlantumlFile = PLANTUML_EXTS.has(ext);
  const isMermaidFile = MERMAID_EXTS.has(ext);
  const isStandaloneDiagram = isPlantumlFile || isMermaidFile;
  const previewRef = useRef<HTMLDivElement>(null);
  const tocResizeObserverRef = useRef<ResizeObserver | null>(null);
  const [isTocSidebarNarrow, setIsTocSidebarNarrow] = useState(false);

  const attachPreviewNode = useCallback((node: HTMLDivElement | null) => {
    previewRef.current = node;
    tocResizeObserverRef.current?.disconnect();
    tocResizeObserverRef.current = null;
    if (!node) return;

    const observer = new ResizeObserver(() => {
      setIsTocSidebarNarrow(
        node.getBoundingClientRect().width <
          window.innerWidth * TOC_SIDEBAR_MIN_WIDTH_RATIO,
      );
    });
    observer.observe(node);
    tocResizeObserverRef.current = observer;
  }, []);

  const { doc, error, parsing } = useAsciiDocRender(
    content,
    /* enabled */ !isStandaloneDiagram,
    docsRoot,
    filePath,
  );

  if (isPlantumlFile) {
    const name = filePath ? (filePath.split(/[/\\]/).pop() ?? null) : null;
    return (
      <div className="asc-preview asc-preview-standalone-plantuml">
        <AscPlantuml block={makeDiagramBlock(content, name)} docsRoot={docsRoot} />
      </div>
    );
  }

  if (isMermaidFile) {
    const name = filePath ? (filePath.split(/[/\\]/).pop() ?? null) : null;
    return (
      <div className="asc-preview">
        <AscMermaid block={makeDiagramBlock(content, name)} docsRoot={docsRoot} />
      </div>
    );
  }

  if (parsing && !doc) {
    return (
      <div className="asc-preview asc-preview-loading">Рендер…</div>
    );
  }

  if (error) {
    return (
      <div className="asc-preview asc-preview-error">
        <div className="asc-preview-error-title">Ошибка парсинга</div>
        <pre className="asc-preview-error-detail">{error}</pre>
      </div>
    );
  }

  if (!doc) {
    return <div className="asc-preview asc-preview-empty">Нет содержимого</div>;
  }

  const docTitle = doc.getDocumentTitle() as string | null;
  const blocks = doc.getBlocks();

  const tocSections = (
    doc.hasSections() ? doc.getSections() : []
  ) as unknown as Section[];
  const tocEnabled = doc.hasAttribute("toc") && tocSections.length > 0;
  const tocPosition = doc.getAttribute("toc-position") as string | null;
  const tocPlacement: AscTocPlacement =
    tocPosition === "left" || tocPosition === "right" ? tocPosition : "top";
  const tocTitle = doc.getAttribute("toc-title", "Table of Contents") as string;
  const tocLevels = Number(doc.getAttribute("toclevels", 2));
  const isSidebarToc =
    tocEnabled && tocPlacement !== "top" && !isTocSidebarNarrow;

  const blockList = (
    <AscBlockList
      blocks={blocks}
      docsRoot={docsRoot}
      monaco={monaco}
      onOpenXref={onOpenXref}
    />
  );

  return (
    <AscPreviewProvider value={{ docsRoot, filePath }}>
      <div className="asc-preview" ref={attachPreviewNode}>
        {docTitle ? (
          <h1 className="asc-doc-title">
            <InlineHtml html={docTitle} onOpenXref={onOpenXref} />
          </h1>
        ) : null}

        {tocEnabled && tocPlacement === "top" ? (
          <AscToc
            sections={tocSections}
            title={tocTitle}
            maxLevel={tocLevels}
            placement="top"
            containerRef={previewRef}
          />
        ) : null}

        {isSidebarToc ? (
          <div className={`asc-preview-body asc-preview-body-toc-${tocPlacement}`}>
            <AscToc
              sections={tocSections}
              title={tocTitle}
              maxLevel={tocLevels}
              placement={tocPlacement}
              containerRef={previewRef}
            />
            <div className="asc-preview-content">{blockList}</div>
          </div>
        ) : (
          blockList
        )}
      </div>
    </AscPreviewProvider>
  );
}
