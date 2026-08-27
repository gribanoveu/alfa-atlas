import { useMemo, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy } from "lucide-react";
import {
  buildJsonLineDiff,
  diffJson,
  formatJsonValue,
  formatUnifiedDiff,
  parseJsonInput,
  summarizeJsonDiff,
  type JsonDiffChange,
} from "../../lib/jsonDiff";
import { UtilityLabeledField } from "./UtilityClearButton";
import "./JsonDiff.css";

const SAMPLE_LEFT = `{
  "id": 1,
  "name": "Alpha",
  "tags": ["docs", "api"]
}`;

const SAMPLE_RIGHT = `{
  "id": 1,
  "name": "Beta",
  "tags": ["docs", "rest"],
  "active": true
}`;

const KIND_LABEL: Record<JsonDiffChange["kind"], string> = {
  add: "Добавлено",
  remove: "Удалено",
  change: "Изменено",
};

function ChangeRow({ change }: { change: JsonDiffChange }) {
  return (
    <div className="json-diff-change">
      <span className={`json-diff-change-kind kind-${change.kind}`}>
        {KIND_LABEL[change.kind]}
      </span>
      <div className="json-diff-change-body">
        <code className="json-diff-change-path">{change.path}</code>
        {change.kind === "add" ? (
          <code className="json-diff-change-value is-to">{formatJsonValue(change.value)}</code>
        ) : null}
        {change.kind === "remove" ? (
          <code className="json-diff-change-value is-from">{formatJsonValue(change.value)}</code>
        ) : null}
        {change.kind === "change" ? (
          <>
            <code className="json-diff-change-value is-from">
              {formatJsonValue(change.from)}
            </code>
            <code className="json-diff-change-value is-to">{formatJsonValue(change.to)}</code>
          </>
        ) : null}
      </div>
    </div>
  );
}

export function JsonDiff() {
  const [leftRaw, setLeftRaw] = useState("");
  const [rightRaw, setRightRaw] = useState("");
  const [copied, setCopied] = useState(false);

  const leftParsed = useMemo(() => parseJsonInput(leftRaw), [leftRaw]);
  const rightParsed = useMemo(() => parseJsonInput(rightRaw), [rightRaw]);

  const result = useMemo(() => {
    if (!leftParsed.ok || !rightParsed.ok) {
      return null;
    }

    const changes = diffJson(leftParsed.value, rightParsed.value);
    const summary = summarizeJsonDiff(changes);
    const lineDiff = buildJsonLineDiff(leftParsed.value, rightParsed.value);

    return {
      changes,
      summary,
      lineDiff,
      unified: formatUnifiedDiff(lineDiff),
      equal: changes.length === 0,
    };
  }, [leftParsed, rightParsed]);

  const handleCopyDiff = async () => {
    if (!result) {
      return;
    }

    try {
      await writeText(result.unified);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Буфер недоступен — diff остаётся на экране.
    }
  };

  const leftError = leftRaw.trim() && !leftParsed.ok ? leftParsed.reason : null;
  const rightError = rightRaw.trim() && !rightParsed.ok ? rightParsed.reason : null;

  return (
    <div className="json-diff">
      <p className="json-diff-desc">
        Сравнение двух JSON-документов: изменения по путям и построчный diff отформатированного
        JSON.
      </p>

      <div className="json-diff-inputs">
        <UtilityLabeledField
          label="Исходный JSON"
          onClear={() => setLeftRaw("")}
          clearDisabled={!leftRaw}
          clearLabel="Очистить исходный JSON"
        >
          <textarea
            className="json-diff-input utility-field-control"
            value={leftRaw}
            onChange={(event) => setLeftRaw(event.target.value)}
            placeholder={SAMPLE_LEFT}
            spellCheck={false}
            aria-label="Исходный JSON"
          />
        </UtilityLabeledField>

        <UtilityLabeledField
          label="Новый JSON"
          onClear={() => setRightRaw("")}
          clearDisabled={!rightRaw}
          clearLabel="Очистить новый JSON"
        >
          <textarea
            className="json-diff-input utility-field-control"
            value={rightRaw}
            onChange={(event) => setRightRaw(event.target.value)}
            placeholder={SAMPLE_RIGHT}
            spellCheck={false}
            aria-label="Новый JSON"
          />
        </UtilityLabeledField>
      </div>

      {leftError ? (
        <p className="json-diff-error" role="status">
          Исходный JSON: {leftError}
        </p>
      ) : null}

      {rightError ? (
        <p className="json-diff-error" role="status">
          Новый JSON: {rightError}
        </p>
      ) : null}

      {result?.equal ? (
        <p className="json-diff-equal" role="status">
          JSON совпадает — различий нет.
        </p>
      ) : null}

      {result && !result.equal ? (
        <>
          <div className="json-diff-summary">
            <span>
              Всего: <strong>{result.summary.total}</strong>
            </span>
            <span className="is-add">
              Добавлено: <strong>{result.summary.added}</strong>
            </span>
            <span className="is-remove">
              Удалено: <strong>{result.summary.removed}</strong>
            </span>
            <span className="is-change">
              Изменено: <strong>{result.summary.changed}</strong>
            </span>
          </div>

          <div className="json-diff-section">
            <h3 className="json-diff-section-title">Изменения по путям</h3>
            <div className="json-diff-changes">
              {result.changes.map((change) => (
                <ChangeRow key={`${change.kind}:${change.path}`} change={change} />
              ))}
            </div>
          </div>

          <div className="json-diff-section">
            <div className="json-diff-section-head">
              <h3 className="json-diff-section-title">Построчный diff</h3>
              <button
                type="button"
                className={`json-diff-copy-btn${copied ? " is-copied" : ""}`}
                onClick={() => void handleCopyDiff()}
                aria-label="Скопировать diff"
                title={copied ? "Скопировано" : "Скопировать diff"}
              >
                {copied ? (
                  <Check size={13} strokeWidth={2} aria-hidden />
                ) : (
                  <Copy size={13} strokeWidth={1.75} aria-hidden />
                )}
              </button>
            </div>
            <pre className="json-diff-lines" aria-label="Построчный diff">
              {result.lineDiff.map((row, index) => (
                <code key={`${row.kind}:${index}:${row.text}`} className={`json-diff-line kind-${row.kind}`}>
                  {row.kind === "add" ? "+ " : row.kind === "remove" ? "- " : "  "}
                  {row.text}
                </code>
              ))}
            </pre>
          </div>
        </>
      ) : null}
    </div>
  );
}
