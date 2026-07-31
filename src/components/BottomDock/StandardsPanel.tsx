import { useMemo, useState } from "react";
import { AlertCircle, CheckCircle2, Loader2, RefreshCw } from "lucide-react";
import type { FolderReport, StandardsReport } from "../../lib/standards";
import type { StandardsCheckStatus } from "../../hooks/useStandardsCheck";
import "./StandardsPanel.css";

type StandardsScope = "current" | "all";

type StandardsPanelProps = {
  report: StandardsReport | null;
  status: StandardsCheckStatus;
  error: string | null;
  /** Docs-root-relative path of the file open in the editor, if any. */
  activeDocsPath: string | null;
  onRunCheck: () => void;
  onOpenSettings: () => void;
};

/** Is `path` the method folder itself, or a file/subpath inside it? */
function folderContainsPath(folder: string, path: string): boolean {
  return path === folder || path.startsWith(`${folder}/`);
}

function FolderGroup({ folder }: { folder: FolderReport }) {
  // Default to showing every criterion (pass and fail alike) so a mixed
  // result is visually obvious at a glance, not just the failing subset.
  const [showAll, setShowAll] = useState(true);
  const failed = folder.findings.filter((f) => !f.passed);
  const visible = showAll ? folder.findings : failed;
  const percent =
    folder.maxScore > 0 ? Math.round((folder.score / folder.maxScore) * 100) : 0;

  return (
    <div className="standards-folder-group">
      <div className="standards-folder-header" title={folder.folder}>
        <span
          className={`standards-folder-status ${folder.passed ? "pass" : "fail"}`}
        >
          {folder.passed ? <CheckCircle2 size={13} /> : <AlertCircle size={13} />}
        </span>
        <span className="standards-folder-name">{folder.folder}</span>
        <span className={`standards-folder-score ${folder.passed ? "pass" : "fail"}`}>
          {percent}%
        </span>
      </div>

      {visible.length === 0 ? (
        <div className="standards-folder-empty">Все включённые критерии пройдены</div>
      ) : (
        <ul className="standards-findings-list">
          {visible.map((finding) => (
            <li key={finding.ruleId} className={finding.passed ? "passed" : "failed"}>
              <div className="standards-finding-head">
                <span
                  className={`standards-finding-icon ${finding.passed ? "pass" : "fail"}`}
                >
                  {finding.passed ? (
                    <CheckCircle2 size={12} />
                  ) : (
                    <AlertCircle size={12} />
                  )}
                </span>
                <span className="standards-finding-rule">{finding.ruleId}</span>
                <span className="standards-finding-title">{finding.title}</span>
                <span className="standards-finding-weight">{finding.weight}</span>
              </div>
              {!finding.passed ? (
                <div className="standards-finding-message">{finding.message}</div>
              ) : null}
            </li>
          ))}
        </ul>
      )}

      {folder.findings.length > failed.length ? (
        <button
          type="button"
          className="standards-toggle-all"
          onClick={() => setShowAll((v) => !v)}
        >
          {showAll ? "Скрыть пройденные критерии" : "Показать все критерии"}
        </button>
      ) : null}
    </div>
  );
}

export function StandardsPanel({
  report,
  status,
  error,
  activeDocsPath,
  onRunCheck,
  onOpenSettings,
}: StandardsPanelProps) {
  const [scope, setScope] = useState<StandardsScope>("current");
  const running = status === "running";
  const passedCount = report?.folders.filter((f) => f.passed).length ?? 0;
  const totalCount = report?.folders.length ?? 0;

  const currentFolder = useMemo(() => {
    if (!report || !activeDocsPath) return null;
    return (
      report.folders.find((f) => folderContainsPath(f.folder, activeDocsPath)) ?? null
    );
  }, [report, activeDocsPath]);

  const visibleFolders = useMemo(() => {
    if (scope === "all") return report?.folders ?? [];
    return currentFolder ? [currentFolder] : [];
  }, [scope, report, currentFolder]);

  let empty: string | null = null;
  if (!report && !running) {
    empty = "Нажмите «Проверить», чтобы оценить соответствие стандартам документации";
  } else if (report && report.folders.length === 0) {
    empty = "Папка документации (src/docs/asciidoc) пуста либо не содержит папок методов";
  } else if (report && scope === "current") {
    if (!activeDocsPath) {
      empty = "Нет открытого файла";
    } else if (!currentFolder) {
      empty = "Текущий файл не относится ни к одной проверенной папке метода";
    }
  }

  return (
    <div className="standards-panel">
      <div className="standards-header">
        <div className="standards-toolbar">
          <div className="standards-toolbar-left">
            {report && report.folders.length > 0 ? (
              <div className="standards-scope" role="tablist" aria-label="Область проверки">
                <button
                  type="button"
                  role="tab"
                  aria-selected={scope === "current"}
                  className={`standards-scope-btn ${scope === "current" ? "active" : ""}`}
                  onClick={() => setScope("current")}
                >
                  Текущий файл
                  <span className="standards-scope-count">{currentFolder ? 1 : 0}</span>
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={scope === "all"}
                  className={`standards-scope-btn ${scope === "all" ? "active" : ""}`}
                  onClick={() => setScope("all")}
                >
                  Весь проект
                  <span className="standards-scope-count">{totalCount}</span>
                </button>
              </div>
            ) : null}
          </div>
          <div className="standards-toolbar-right">
            <button
              type="button"
              className="standards-run-btn"
              onClick={onRunCheck}
              disabled={running}
            >
              {running ? (
                <Loader2 size={13} className="standards-spin" />
              ) : (
                <RefreshCw size={13} />
              )}
              Проверить
            </button>
            <button
              type="button"
              className="standards-settings-btn"
              onClick={onOpenSettings}
            >
              Настроить правила
            </button>
          </div>
        </div>

        {report ? (
          <div className="standards-summary-row">
            <span className="standards-summary">
              {totalCount === 0
                ? "Папки с документацией не найдены"
                : `${passedCount} из ${totalCount} папок соответствуют стандарту (>80%)`}
            </span>
          </div>
        ) : null}

        {error ? <div className="standards-error">{error}</div> : null}
      </div>

      {empty !== null ? (
        <div className="panel-empty">{empty}</div>
      ) : (
        <div className="standards-scroll">
          <div className="standards-folders">
            {visibleFolders.map((folder) => (
              <FolderGroup key={folder.folder} folder={folder} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
