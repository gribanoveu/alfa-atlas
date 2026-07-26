import type * as Monaco from "monaco-editor";
import { useAsciiDocRender } from "../../hooks/useAsciiDocRender";
import { AscBlockList } from "./AscBlockList";
import { InlineHtml } from "./InlineHtml";
import "./AsciiDocPreview.css";

type AsciiDocPreviewProps = {
  content: string;
  docsRoot: string | null;
  /** Monaco namespace — нужен для подсветки кода в AscCodeBlock. */
  monaco: typeof Monaco | null;
};

/**
 * Контейнер превью AsciiDoc: парсит контент в AST и рендерит дерево блоков
 * React-компонентами проекта. Состояния: загрузка, ошибка парсинга, пусто,
 * готово.
 */
export function AsciiDocPreview({
  content,
  docsRoot,
  monaco,
}: AsciiDocPreviewProps) {
  const { doc, error, parsing } = useAsciiDocRender(content, true, docsRoot);

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
          <InlineHtml html={docTitle} />
        </h1>
      ) : null}
      <AscBlockList blocks={blocks} docsRoot={docsRoot} monaco={monaco} />
    </div>
  );
}
