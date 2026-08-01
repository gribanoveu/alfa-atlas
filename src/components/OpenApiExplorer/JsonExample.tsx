import { useEffect, useMemo, useState } from "react";
import * as monaco from "monaco-editor";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { ATLAS_DARK_THEME_ID } from "../../monaco/asciidocLanguage";
import { skeletonForSchema } from "./requestBuilder";
import "./OpenApiExplorer.css";

type JsonExampleProps = {
  schema: unknown;
};

/** Always-visible, copyable, syntax-highlighted example JSON generated from
 * a schema — meant to be handed off as a contract to a backend/frontend
 * team or pasted into other docs, not just read on-screen like
 * `SchemaViewer`'s type tree. Highlighting uses Monaco's static `colorize`
 * API (same mechanism as fenced code blocks in Markdown preview) rather
 * than mounting a full editor instance per block. */
export function JsonExample({ schema }: JsonExampleProps) {
  const [copied, setCopied] = useState(false);
  const [html, setHtml] = useState<string | null>(null);

  const json = useMemo(
    () => JSON.stringify(skeletonForSchema(schema), null, 2),
    [schema],
  );

  useEffect(() => {
    let cancelled = false;
    // Ensures the app's dark theme is active even if this is the first
    // Monaco-related thing rendered in the session (colorize() otherwise
    // falls back to Monaco's default light theme).
    monaco.editor.setTheme(ATLAS_DARK_THEME_ID);
    monaco.editor
      .colorize(json, "json", { tabSize: 2 })
      .then((out) => {
        if (!cancelled) setHtml(out);
      })
      .catch(() => {
        if (!cancelled) setHtml(null);
      });
    return () => {
      cancelled = true;
    };
  }, [json]);

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
        <span className="oas-json-example-title">Итоговый JSON</span>
        <button
          type="button"
          className="oas-try-copy-btn"
          onClick={() => void handleCopy()}
        >
          {copied ? "Скопировано" : "Копировать"}
        </button>
      </div>
      <pre className="oas-json-example-code">
        {html ? (
          <code dangerouslySetInnerHTML={{ __html: html }} />
        ) : (
          <code>{json}</code>
        )}
      </pre>
    </div>
  );
}
