import { useMemo, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy } from "lucide-react";
import {
  formatJsonInput,
  type JsonFormatMode,
  type JsonIndent,
} from "../../lib/jsonFormat";
import { UtilityLabeledField } from "./UtilityClearButton";
import "./JsonFormatter.css";

const SAMPLE = '{"id":1,"name":"Alpha","tags":["docs","api"],"meta":{"version":2}}';

const MODES: { id: JsonFormatMode; label: string }[] = [
  { id: "prettify", label: "Prettify" },
  { id: "minify", label: "Minify" },
];

const INDENTS: JsonIndent[] = [2, 4];

function formatBytes(size: number): string {
  if (size < 1024) {
    return `${size} B`;
  }
  return `${(size / 1024).toFixed(1)} KB`;
}

export function JsonFormatter() {
  const [raw, setRaw] = useState("");
  const [mode, setMode] = useState<JsonFormatMode>("prettify");
  const [indent, setIndent] = useState<JsonIndent>(2);
  const [sortKeys, setSortKeys] = useState(false);
  const [copied, setCopied] = useState(false);

  const formatted = useMemo(
    () => formatJsonInput(raw, { mode, indent, sortKeys }),
    [raw, mode, indent, sortKeys],
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
    <div className="jsonfmt">
      <p className="jsonfmt-desc">
        Форматирование JSON: prettify с отступами, minify в одну строку и опциональная
        сортировка ключей.
      </p>

      <div className="jsonfmt-toolbar">
        <div className="jsonfmt-group">
          <span className="jsonfmt-group-label">Режим</span>
          <div className="jsonfmt-segmented" role="tablist" aria-label="Режим форматирования">
            {MODES.map((item) => (
              <button
                key={item.id}
                type="button"
                role="tab"
                aria-selected={mode === item.id}
                className={`jsonfmt-segment${mode === item.id ? " is-active" : ""}`}
                onClick={() => setMode(item.id)}
              >
                {item.label}
              </button>
            ))}
          </div>
        </div>

        <div className="jsonfmt-group">
          <span className="jsonfmt-group-label">Отступ</span>
          <div className="jsonfmt-segmented" role="tablist" aria-label="Размер отступа">
            {INDENTS.map((value) => (
              <button
                key={value}
                type="button"
                role="tab"
                aria-selected={indent === value}
                className={`jsonfmt-segment${indent === value ? " is-active" : ""}`}
                disabled={mode === "minify"}
                onClick={() => setIndent(value)}
              >
                {value}
              </button>
            ))}
          </div>
        </div>

        <div className="jsonfmt-group">
          <span className="jsonfmt-group-label">Сортировать</span>
          <div className="jsonfmt-segmented" role="tablist" aria-label="Сортировка ключей">
            <button
              type="button"
              role="tab"
              aria-selected={!sortKeys}
              className={`jsonfmt-segment${!sortKeys ? " is-active" : ""}`}
              onClick={() => setSortKeys(false)}
            >
              Нет
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={sortKeys}
              className={`jsonfmt-segment${sortKeys ? " is-active" : ""}`}
              onClick={() => setSortKeys(true)}
            >
              Да
            </button>
          </div>
        </div>
      </div>

      <UtilityLabeledField
        label="JSON"
        onClear={() => setRaw("")}
        clearDisabled={!raw}
        clearLabel="Очистить JSON"
      >
        <textarea
          className="jsonfmt-input utility-field-control"
          value={raw}
          onChange={(event) => setRaw(event.target.value)}
          placeholder={SAMPLE}
          spellCheck={false}
          aria-label="JSON"
        />
      </UtilityLabeledField>

      {showError ? (
        <p className="jsonfmt-error" role="status">
          {formatted.reason}
        </p>
      ) : null}

      {formatted.ok && raw.trim() ? (
        <>
          <div className="jsonfmt-meta">
            <span>
              Вход: <strong>{formatBytes(formatted.bytesIn)}</strong>
            </span>
            <span>
              Выход: <strong>{formatBytes(formatted.bytesOut)}</strong>
            </span>
          </div>

          <div className="jsonfmt-actions">
            <button type="button" className="jsonfmt-action" onClick={handleApply}>
              Заменить вход
            </button>
          </div>

          <div className="jsonfmt-output-block">
            <div className="jsonfmt-output-head">
              <h3 className="jsonfmt-section-title">Результат</h3>
              <button
                type="button"
                className={`jsonfmt-copy-btn${copied ? " is-copied" : ""}`}
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
            <pre className="jsonfmt-output" aria-label="Результат форматирования">
              {formatted.output}
            </pre>
          </div>
        </>
      ) : null}
    </div>
  );
}
