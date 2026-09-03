import { useMemo, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { highlightJson } from "./jsonHighlight";
import { skeletonForSchema } from "./requestBuilder";
import "../StructuredDataPreview/StructuredDataPreview.css";
import "./OpenApiExplorer.css";

type JsonExampleProps = {
  /** Схема, из которой генерируется «болванка» ответа/запроса. */
  schema?: unknown;
  /** Готовое значение (именованный пример из спеки) — показывается как есть. */
  value?: unknown;
  title?: string;
};

/** Always-visible, copyable, syntax-highlighted example JSON generated from
 * a schema — meant to be handed off as a contract to a backend/frontend
 * team or pasted into other docs, not just read on-screen like
 * `SchemaViewer`'s type tree. */
export function JsonExample({ schema, value, title }: JsonExampleProps) {
  const [copied, setCopied] = useState(false);

  const json = useMemo(
    () =>
      JSON.stringify(value !== undefined ? value : skeletonForSchema(schema), null, 2),
    [schema, value],
  );

  const html = useMemo(() => highlightJson(json), [json]);

  const handleCopy = async () => {
    try {
      await writeText(json);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard unavailable — silently ignore, JSON text is still visible.
    }
  };

  return (
    <div className="oas-json-example">
      <div className="oas-json-example-header">
        <span className="oas-json-example-title">{title ?? "Итоговый JSON"}</span>
        <button
          type="button"
          className="oas-try-copy-btn"
          onClick={() => void handleCopy()}
        >
          {copied ? "Скопировано" : "Копировать"}
        </button>
      </div>
      <pre className="oas-json-example-code">
        <code dangerouslySetInnerHTML={{ __html: html }} />
      </pre>
    </div>
  );
}
