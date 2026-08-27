import { useMemo, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy } from "lucide-react";
import {
  jwtClaimEntries,
  jwtSummary,
  parseJwt,
} from "../../lib/jwt";
import { UtilityInputHead } from "./UtilityClearButton";
import "./JwtParser.css";

const SAMPLE =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2HT4EpwuHnKz-zZX0";

function CopyButton({
  id,
  value,
  copiedId,
  onCopy,
  label,
}: {
  id: string;
  value: string;
  copiedId: string | null;
  onCopy: (id: string, value: string) => void;
  label: string;
}) {
  const copied = copiedId === id;
  return (
    <button
      type="button"
      className={`jwt-copy-btn${copied ? " is-copied" : ""}`}
      onClick={() => onCopy(id, value)}
      aria-label={label}
      title={copied ? "Скопировано" : "Копировать"}
    >
      {copied ? (
        <Check size={13} strokeWidth={2} aria-hidden />
      ) : (
        <Copy size={13} strokeWidth={1.75} aria-hidden />
      )}
    </button>
  );
}

function JsonBlock({
  title,
  json,
  copyId,
  copiedId,
  onCopy,
}: {
  title: string;
  json: string;
  copyId: string;
  copiedId: string | null;
  onCopy: (id: string, value: string) => void;
}) {
  return (
    <div className="jwt-block">
      <div className="jwt-block-head">
        <h3 className="jwt-section-title">{title}</h3>
        <CopyButton
          id={copyId}
          value={json}
          copiedId={copiedId}
          onCopy={onCopy}
          label={`Скопировать: ${title}`}
        />
      </div>
      <pre className="jwt-pre">{json}</pre>
    </div>
  );
}

export function JwtParser() {
  const [raw, setRaw] = useState("");
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const parsed = useMemo(() => parseJwt(raw), [raw]);

  const handleCopy = async (id: string, value: string) => {
    try {
      await writeText(value);
      setCopiedId(id);
      setTimeout(() => setCopiedId((current) => (current === id ? null : current)), 1500);
    } catch {
      // Буфер недоступен — значение всё равно видно на экране и выделяется мышью.
    }
  };

  return (
    <div className="jwt-parser">
      <p className="jwt-panel-desc">
        Разбор заголовка и payload. Подпись не проверяется.
      </p>

      <div className="jwt-input-wrap">
        <UtilityInputHead label="JWT" onClear={() => setRaw("")} clearDisabled={!raw} />
        <textarea
          className="jwt-input"
          value={raw}
          onChange={(event) => setRaw(event.target.value)}
          placeholder={SAMPLE}
          spellCheck={false}
          aria-label="JWT"
        />
      </div>

      {parsed.ok ? (
        <>
          <div className="jwt-summary">
            <span>
              Алгоритм: <strong>{jwtSummary(parsed.value).alg}</strong>
            </span>
            <span>
              Тип: <strong>{jwtSummary(parsed.value).typ}</strong>
            </span>
          </div>

          <JsonBlock
            title="Header"
            json={parsed.value.headerJson}
            copyId="header"
            copiedId={copiedId}
            onCopy={handleCopy}
          />

          <JsonBlock
            title="Payload"
            json={parsed.value.payloadJson}
            copyId="payload"
            copiedId={copiedId}
            onCopy={handleCopy}
          />

          {jwtClaimEntries(parsed.value.payload).length > 0 ? (
            <div className="jwt-block">
              <h3 className="jwt-section-title">Claims</h3>
              <div className="jwt-rows">
                {jwtClaimEntries(parsed.value.payload).map((claim) => (
                  <div key={claim.key} className="jwt-row">
                    <span className="jwt-row-label">{claim.key}</span>
                    <code className="jwt-row-value">{claim.value}</code>
                  </div>
                ))}
              </div>
            </div>
          ) : null}

          <div className="jwt-block">
            <div className="jwt-block-head">
              <h3 className="jwt-section-title">Signature</h3>
              <CopyButton
                id="signature"
                value={parsed.value.signature}
                copiedId={copiedId}
                onCopy={handleCopy}
                label="Скопировать: Signature"
              />
            </div>
            <pre className="jwt-pre">{parsed.value.signature}</pre>
          </div>
        </>
      ) : raw.trim() ? (
        <p className="jwt-error" role="status">
          {parsed.reason}
        </p>
      ) : null}
    </div>
  );
}
