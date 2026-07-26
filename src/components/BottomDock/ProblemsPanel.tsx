import { useMemo } from "react";
import { AlertCircle, AlertTriangle } from "lucide-react";
import type { Diagnostic } from "../../lib/workspaceIndex";
import "./ProblemsPanel.css";

type ProblemsPanelProps = {
  diagnostics: Diagnostic[];
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
  onOpenDiagnostic,
}: ProblemsPanelProps) {
  const groups = useMemo(() => groupByDocument(diagnostics), [diagnostics]);

  if (diagnostics.length === 0) {
    return <div className="panel-empty">Нет проблем в индексе</div>;
  }

  return (
    <div className="problems-panel">
      {groups.map((group) => (
        <div key={group.document} className="problems-file-group">
          <div className="problems-file-header" title={group.document}>
            <span className="problems-file-name">{group.document}</span>
            <span className="problems-file-count">{group.items.length}</span>
          </div>
          <ul className="problems-list">
            {group.items.map((d, i) => {
              const Icon =
                d.severity === "error" ? AlertCircle : AlertTriangle;
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
      ))}
    </div>
  );
}
