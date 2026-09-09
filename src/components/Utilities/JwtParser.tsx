import { useMemo, useState } from "react";
import {
  jwtClaimEntries,
  jwtSummary,
  parseJwt,
} from "../../lib/jwt";
import { UtilityLabeledField } from "./UtilityClearButton";
import { CopyTextButton } from "../Common/CopyTextButton";
import "./JwtParser.css";

const SAMPLE =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2HT4EpwuHnKz-zZX0";

// No `copiedId`/`onCopy` pair to thread down any more: which button is
// ticked is that button's own business (see `CopyTextButton`).
function JsonBlock({ title, json }: { title: string; json: string }) {
  return (
    <div className="jwt-block">
      <div className="jwt-block-head">
        <h3 className="jwt-section-title">{title}</h3>
        <CopyTextButton
          text={json}
          className="jwt-copy-btn"
          label="Копировать"
          ariaLabel={`Скопировать: ${title}`}
          size={13}
        />
      </div>
      <pre className="jwt-pre">{json}</pre>
    </div>
  );
}

export function JwtParser() {
  const [raw, setRaw] = useState("");

  const parsed = useMemo(() => parseJwt(raw), [raw]);

  return (
    <div className="jwt-parser">
      <p className="jwt-panel-desc">
        Разбор заголовка и payload. Подпись не проверяется.
      </p>

      <UtilityLabeledField
        label="JWT"
        onClear={() => setRaw("")}
        clearDisabled={!raw}
        clearLabel="Очистить JWT"
      >
        <textarea
          className="jwt-input utility-field-control"
          value={raw}
          onChange={(event) => setRaw(event.target.value)}
          placeholder={SAMPLE}
          spellCheck={false}
          aria-label="JWT"
        />
      </UtilityLabeledField>

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

          <JsonBlock title="Header" json={parsed.value.headerJson} />

          <JsonBlock title="Payload" json={parsed.value.payloadJson} />

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
              <CopyTextButton
                text={parsed.value.signature}
                className="jwt-copy-btn"
                label="Копировать"
                ariaLabel="Скопировать: Signature"
                size={13}
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
