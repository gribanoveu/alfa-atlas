import { useMemo, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy } from "lucide-react";
import {
  formatXmlInput,
  type XmlFormatMode,
  type XmlIndent,
} from "../../lib/xmlFormat";
import "./XmlFormatter.css";

const SAMPLE = `<root><item id="1"><name>Alpha</name><tags><tag>docs</tag><tag>api</tag></tags></item></root>`;

const MODES: { id: XmlFormatMode; label: string }[] = [
  { id: "prettify", label: "Prettify" },
  { id: "minify", label: "Minify" },
];

const INDENTS: XmlIndent[] = [2, 4];

function formatBytes(size: number): string {
  if (size < 1024) {
    return `${size} B`;
  }
  return `${(size / 1024).toFixed(1)} KB`;
}

export function XmlFormatter() {
  const [raw, setRaw] = useState("");
  const [mode, setMode] = useState<XmlFormatMode>("prettify");
  const [indent, setIndent] = useState<XmlIndent>(2);
  const [copied, setCopied] = useState(false);

  const formatted = useMemo(
    () => formatXmlInput(raw, { mode, indent }),
    [raw, mode, indent],
  );

  const handleCopy = async () => {
    if (!formatted.ok) {
      return;
    }

    try {
      await writeText(formatted.output);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Буфер недоступен — результат остаётся на экране.
    }
  };

  const handleApply = () => {
    if (!formatted.ok) {
      return;
    }
    setRaw(formatted.output);
  };

  const showError = raw.trim().length > 0 && !formatted.ok;

  return (
    <div className="xmfmt">
      <p className="xmfmt-desc">
        Форматирование XML: prettify с отступами и minify в одну строку. Учитывается
        атрибут xml:space=&quot;preserve&quot;.
      </p>

      <div className="xmfmt-toolbar">
        <div className="xmfmt-group">
          <span className="xmfmt-group-label">Режим</span>
          <div className="xmfmt-segmented" role="tablist" aria-label="Режим форматирования">
            {MODES.map((item) => (
              <button
                key={item.id}
                type="button"
                role="tab"
                aria-selected={mode === item.id}
                className={`xmfmt-segment${mode === item.id ? " is-active" : ""}`}
                onClick={() => setMode(item.id)}
              >
                {item.label}
              </button>
            ))}
          </div>
        </div>

        <div className="xmfmt-group">
          <span className="xmfmt-group-label">Отступ</span>
          <div className="xmfmt-segmented" role="tablist" aria-label="Размер отступа">
            {INDENTS.map((value) => (
              <button
                key={value}
                type="button"
                role="tab"
                aria-selected={indent === value}
                className={`xmfmt-segment${indent === value ? " is-active" : ""}`}
                disabled={mode === "minify"}
                onClick={() => setIndent(value)}
              >
                {value}
              </button>
            ))}
          </div>
        </div>
      </div>

      <label className="xmfmt-input-wrap">
        <span className="xmfmt-input-label">XML</span>
        <textarea
          className="xmfmt-input"
          value={raw}
          onChange={(event) => setRaw(event.target.value)}
          placeholder={SAMPLE}
          spellCheck={false}
          aria-label="XML"
        />
      </label>

      {showError ? (
        <p className="xmfmt-error" role="status">
          {formatted.reason}
        </p>
      ) : null}

      {formatted.ok && raw.trim() ? (
        <>
          <div className="xmfmt-meta">
            <span>
              Вход: <strong>{formatBytes(formatted.bytesIn)}</strong>
            </span>
            <span>
              Выход: <strong>{formatBytes(formatted.bytesOut)}</strong>
            </span>
          </div>

          <div className="xmfmt-actions">
            <button type="button" className="xmfmt-action" onClick={handleApply}>
              Заменить вход
            </button>
          </div>

          <div className="xmfmt-output-block">
            <div className="xmfmt-output-head">
              <h3 className="xmfmt-section-title">Результат</h3>
              <button
                type="button"
                className={`xmfmt-copy-btn${copied ? " is-copied" : ""}`}
                onClick={() => void handleCopy()}
                aria-label="Скопировать результат"
                title={copied ? "Скопировано" : "Скопировать результат"}
              >
                {copied ? (
                  <Check size={13} strokeWidth={2} aria-hidden />
                ) : (
                  <Copy size={13} strokeWidth={1.75} aria-hidden />
                )}
              </button>
            </div>
            <pre className="xmfmt-output" aria-label="Результат форматирования">
              {formatted.output}
            </pre>
          </div>
        </>
      ) : null}
    </div>
  );
}
