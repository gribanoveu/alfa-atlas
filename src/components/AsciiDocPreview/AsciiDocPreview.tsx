import type * as Monaco from "monaco-editor";
import { useAsciiDocRender } from "../../hooks/useAsciiDocRender";
import { AscBlockList } from "./AscBlockList";
import { AscMermaid } from "./AscMermaid";
import { AscPlantuml } from "./AscPlantuml";
import { InlineHtml } from "./InlineHtml";
import type { AbstractBlock } from "./types";
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

function extensionOf(path: string): string {
  const base = path.split(/[/\\]/).pop() ?? path;
  const dot = base.lastIndexOf(".");
  return dot <= 0 ? "" : base.slice(dot).toLowerCase();
}

/**
 * Standalone `.puml` file → fake asciidoctor "listing" block whose
 * `getSource()` returns the raw PlantUML source. `AscPlantuml` renders it
 * through the vendored TeaVM engine, identical to `[plantuml] ---- … ----`
 * blocks embedded in `.adoc`.
 */
function makePlantumlBlock(source: string, name: string | null): AbstractBlock {
  return {
    getSource: () => source,
    getAttribute: (key: string) => (key === "1" ? name : null),
  } as unknown as AbstractBlock;
}

function makeMermaidBlock(source: string, name: string | null): AbstractBlock {
  return {
    getSource: () => source,
    getAttribute: (key: string) => (key === "1" ? name : null),
  } as unknown as AbstractBlock;
}

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

  const { doc, error, parsing } = useAsciiDocRender(
    content,
    /* enabled */ !isStandaloneDiagram,
    docsRoot,
    filePath,
  );

  if (isPlantumlFile) {
    const name = filePath ? (filePath.split(/[/\\]/).pop() ?? null) : null;
    return (
      <div className="asc-preview">
        <AscPlantuml block={makePlantumlBlock(content, name)} docsRoot={docsRoot} />
      </div>
    );
  }

  if (isMermaidFile) {
    const name = filePath ? (filePath.split(/[/\\]/).pop() ?? null) : null;
    return (
      <div className="asc-preview">
        <AscMermaid block={makeMermaidBlock(content, name)} docsRoot={docsRoot} />
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

  return (
    <div className="asc-preview">
      {docTitle ? (
        <h1 className="asc-doc-title">
          <InlineHtml html={docTitle} onOpenXref={onOpenXref} />
        </h1>
      ) : null}
      <AscBlockList
        blocks={blocks}
        docsRoot={docsRoot}
        monaco={monaco}
        onOpenXref={onOpenXref}
      />
    </div>
  );
}
