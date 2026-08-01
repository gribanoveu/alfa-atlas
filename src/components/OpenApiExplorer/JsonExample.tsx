import { useMemo, useState } from "react";
import { Braces } from "lucide-react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { skeletonForSchema } from "./requestBuilder";
import "./OpenApiExplorer.css";

type JsonExampleProps = {
  schema: unknown;
};

/** Collapsible, copyable example JSON generated from a schema — meant to be
 * handed off as a contract to a backend/frontend team or pasted into other
 * docs, not just read on-screen like `SchemaViewer`'s type tree. */
export function JsonExample({ schema }: JsonExampleProps) {
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);

  const json = useMemo(
    () => JSON.stringify(skeletonForSchema(schema), null, 2),
    [schema],
  );

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
      <button
        type="button"
        className={`oas-json-example-toggle ${open ? "active" : ""}`}
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
      >
        <Braces size={13} aria-hidden />
        Итоговый JSON
        <span className="oas-json-example-chevron" aria-hidden>
          {open ? "▾" : "▸"}
        </span>
      </button>
      {open ? (
        <div className="oas-json-example-body">
          <div className="oas-json-example-actions">
            <button
              type="button"
              className="oas-try-copy-btn"
              onClick={() => void handleCopy()}
            >
              {copied ? "Скопировано" : "Копировать"}
            </button>
          </div>
          <pre className="oas-try-response-body">{json}</pre>
        </div>
      ) : null}
    </div>
  );
}
