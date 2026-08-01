import { useState } from "react";
import type { RefDiagnostic } from "../../lib/openapi";
import "./OpenApiExplorer.css";

type DiagnosticsBannerProps = {
  diagnostics: RefDiagnostic[];
};

export function DiagnosticsBanner({ diagnostics }: DiagnosticsBannerProps) {
  const [expanded, setExpanded] = useState(false);
  if (diagnostics.length === 0) return null;

  return (
    <div className="oas-diagnostics">
      <button
        type="button"
        className="oas-diagnostics-summary"
        onClick={() => setExpanded((e) => !e)}
      >
        {expanded ? "▾" : "▸"} Нерешённых ссылок: {diagnostics.length}
      </button>
      {expanded ? (
        <ul className="oas-diagnostics-list">
          {diagnostics.map((d, i) => (
            <li key={i}>
              <code>{d.ref}</code> — {d.reason} (в {d.referencedFrom})
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
