import {
  FileInput,
  Image,
  Link2,
  Minus,
} from "lucide-react";
import type { AsciiDocSnippet } from "../../lib/asciidocSnippets";
import "./SnippetThumbnail.css";

type SnippetThumbnailProps = {
  snippet: AsciiDocSnippet;
};

/** Краткая подсказка по синтаксису — показывается под макетом. */
const SNIPPET_SYNTAX: Record<string, string> = {
  "doc-title": "=",
  "doc-attrs": ":toc: left",
  section: "==",
  subsection: "===",
  anchor: "[[id]]",
  ulist: "* пункт",
  olist: ". пункт",
  "thematic-break": "'''",
  "http-method": "[cols]",
  "simple-table": "|===|",
  "request-params": "|===|",
  "response-fields": "|===|",
  "error-codes": "|===|",
  "source-code": "[source]",
  "source-json": "[source,json]",
  quote: "[quote]",
  note: "NOTE:",
  tip: "TIP:",
  warning: "WARNING:",
  important: "IMPORTANT:",
  image: "image::",
  xref: "xref:",
  link: "https://",
  include: "include::",
};

function PreviewAdmonition({
  kind,
  label,
}: {
  kind: "note" | "tip" | "warning" | "important";
  label: string;
}) {
  return (
    <div className={`sp-admonition sp-admonition-${kind}`}>
      <span className="sp-admonition-label">{label}</span>
      <span className="sp-admonition-text">Текст блока</span>
    </div>
  );
}

function PreviewTable({
  headers,
  rows,
}: {
  headers: string[];
  rows: string[][];
}) {
  return (
    <table className="sp-table">
      <thead>
        <tr>
          {headers.map((h) => (
            <th key={h}>{h}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((row, i) => (
          <tr key={i}>
            {row.map((cell, j) => (
              <td key={j}>{cell}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function SnippetVisual({ id }: { id: string }) {
  switch (id) {
    case "doc-title":
      return (
        <div className="sp-block">
          <div className="sp-heading sp-h1">Заголовок</div>
          <div className="sp-line" />
        </div>
      );
    case "doc-attrs":
      return (
        <div className="sp-doc-attrs">
          <aside className="sp-doc-attrs-toc">
            <span className="sp-doc-attrs-toc-title">Оглавление</span>
            <span>1. Раздел</span>
            <span>2. Раздел</span>
          </aside>
          <div className="sp-doc-attrs-body">
            <span className="sp-heading sp-h3">1. Раздел</span>
            <div className="sp-line sp-line-short" />
          </div>
        </div>
      );
    case "section":
      return (
        <div className="sp-block">
          <div className="sp-heading sp-h2">Раздел</div>
          <div className="sp-line sp-line-short" />
        </div>
      );
    case "subsection":
      return (
        <div className="sp-block">
          <div className="sp-heading sp-h3">Подраздел</div>
          <div className="sp-line sp-line-short" />
        </div>
      );
    case "anchor":
      return (
        <div className="sp-anchor">
          <code>[[section-id]]</code>
        </div>
      );
    case "ulist":
      return (
        <ul className="sp-list sp-ulist">
          <li>Пункт</li>
          <li>Пункт</li>
        </ul>
      );
    case "olist":
      return (
        <ol className="sp-list sp-olist">
          <li>Пункт</li>
          <li>Пункт</li>
        </ol>
      );
    case "thematic-break":
      return (
        <div className="sp-block sp-break">
          <div className="sp-line sp-line-short" />
          <Minus className="sp-break-icon" size={14} aria-hidden />
          <div className="sp-line sp-line-short" />
        </div>
      );
    case "http-method":
      return (
        <PreviewTable
          headers={["Метод", "POST"]}
          rows={[["URL", "/api/…"]]}
        />
      );
    case "simple-table":
      return (
        <PreviewTable
          headers={["A", "B"]}
          rows={[["1", "2"]]}
        />
      );
    case "request-params":
      return (
        <PreviewTable
          headers={["Параметр", "Тип", "Обяз.", "Описание"]}
          rows={[["name", "string", "Да", "…"]]}
        />
      );
    case "response-fields":
      return (
        <PreviewTable
          headers={["Поле", "Тип", "Описание"]}
          rows={[["field", "string", "…"]]}
        />
      );
    case "error-codes":
      return (
        <PreviewTable
          headers={["Код", "Описание"]}
          rows={[["ERR_01", "…"]]}
        />
      );
    case "source-code":
      return (
        <pre className="sp-code">
          <span className="sp-code-lang">source</span>
          <code>код</code>
        </pre>
      );
    case "source-json":
      return (
        <pre className="sp-code sp-code-json">
          <span className="sp-code-lang">json</span>
          <code>{`{ "key": "…" }`}</code>
        </pre>
      );
    case "quote":
      return (
        <blockquote className="sp-quote">Текст цитаты</blockquote>
      );
    case "note":
      return <PreviewAdmonition kind="note" label="NOTE" />;
    case "tip":
      return <PreviewAdmonition kind="tip" label="TIP" />;
    case "warning":
      return <PreviewAdmonition kind="warning" label="WARNING" />;
    case "important":
      return <PreviewAdmonition kind="important" label="IMPORTANT" />;
    case "image":
      return (
        <div className="sp-image">
          <Image size={22} strokeWidth={1.5} aria-hidden />
          <span>image.png</span>
        </div>
      );
    case "xref":
      return (
        <div className="sp-link">
          <Link2 size={12} aria-hidden />
          <span className="sp-link-text">xref:doc.adoc</span>
        </div>
      );
    case "link":
      return (
        <div className="sp-link">
          <Link2 size={12} aria-hidden />
          <span className="sp-link-text">example.com</span>
        </div>
      );
    case "include":
      return (
        <div className="sp-include">
          <FileInput size={18} strokeWidth={1.5} aria-hidden />
          <code>include::file.adoc[]</code>
        </div>
      );
    default:
      return (
        <div className="sp-block">
          <div className="sp-line" />
          <div className="sp-line sp-line-short" />
        </div>
      );
  }
}

export function SnippetThumbnail({ snippet }: SnippetThumbnailProps) {
  const syntax = SNIPPET_SYNTAX[snippet.id] ?? snippet.description ?? "";

  return (
    <div className="adoc-snippet-preview" aria-hidden>
      <div className="adoc-snippet-preview-visual">
        <SnippetVisual id={snippet.id} />
      </div>
      {syntax ? (
        <div className="adoc-snippet-preview-syntax">
          <code>{syntax}</code>
        </div>
      ) : null}
    </div>
  );
}
