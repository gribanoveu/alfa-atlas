import { useMemo, useState } from "react";
import { AlertCircle, AlertTriangle, Info } from "lucide-react";
import type { Diagnostic } from "../../lib/workspaceIndex";
import "./ProblemsPanel.css";

type ProblemsScope = "current" | "all";

type ProblemsPanelProps = {
  diagnostics: Diagnostic[];
  activeDocumentId: string | null;
  onOpenDiagnostic: (
    documentId: string,
    line: number,
    column: number,
  ) => void;
};

type DiagnosticGroup = {
  document: string;
  items: Diagnostic[];
};

function groupByDocument(diagnostics: Diagnostic[]): DiagnosticGroup[] {
  const map = new Map<string, Diagnostic[]>();
  for (const d of diagnostics) {
    const list = map.get(d.document);
    if (list) list.push(d);
    else map.set(d.document, [d]);
  }
  // Sort documents alphabetically; items keep their original order (already
  // sorted by line/col on the backend).
  return [...map.entries()]
    .map(([document, items]) => ({ document, items }))
    .sort((a, b) => a.document.localeCompare(b.document));
}

export function ProblemsPanel({
  diagnostics,
  activeDocumentId,
  onOpenDiagnostic,
}: ProblemsPanelProps) {
  const [scope, setScope] = useState<ProblemsScope>("current");

  const visibleDiagnostics = useMemo(() => {
    if (scope === "all") return diagnostics;
    if (!activeDocumentId) return [];
    return diagnostics.filter((d) => d.document === activeDocumentId);
  }, [diagnostics, scope, activeDocumentId]);

  const groups = useMemo(
    () => groupByDocument(visibleDiagnostics),
    [visibleDiagnostics],
  );

  const currentCount = activeDocumentId
    ? diagnostics.filter((d) => d.document === activeDocumentId).length
    : 0;
  const allCount = diagnostics.length;

  let empty: string | null = null;
  if (visibleDiagnostics.length === 0) {
    empty =
      scope === "current"
        ? activeDocumentId
          ? "Нет проблем в текущем файле"
          : "Нет открытого файла"
        : "Нет проблем в индексе";
  }

  return (
    <div className="problems-panel">
      <div className="problems-scope" role="tablist" aria-label="Область проблем">
        <button
          type="button"
          role="tab"
          aria-selected={scope === "current"}
          className={`problems-scope-btn ${scope === "current" ? "active" : ""}`}
          onClick={() => setScope("current")}
        >
          Текущий файл
          <span className="problems-scope-count">{currentCount}</span>
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={scope === "all"}
          className={`problems-scope-btn ${scope === "all" ? "active" : ""}`}
          onClick={() => setScope("all")}
        >
          Все
          <span className="problems-scope-count">{allCount}</span>
        </button>
      </div>

      {empty !== null ? (
        <div className="panel-empty">{empty}</div>
      ) : (
        groups.map((group) => (
          <div key={group.document} className="problems-file-group">
            <div className="problems-file-header" title={group.document}>
              <span className="problems-file-name">{group.document}</span>
              <span className="problems-file-count">{group.items.length}</span>
            </div>
            <ul className="problems-list">
              {group.items.map((d, i) => {
                const Icon =
                  d.severity === "error"
                    ? AlertCircle
                    : d.severity === "warning"
                      ? AlertTriangle
                      : Info;
                return (
                  <li key={`${d.document}-${d.line}-${d.column}-${i}`}>
                    <button
                      type="button"
                      className={`problems-item ${d.severity}`}
                      onClick={() =>
                        onOpenDiagnostic(d.document, d.line, d.column)
                      }
                      title={`${d.kind}: ${d.message}`}
                    >
                      <Icon size={12} className="problems-item-icon" />
                      <span className="problems-item-message">{d.message}</span>
                      <span className="problems-item-loc">
                        {d.line}:{d.column}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        ))
      )}
    </div>
  );
}
