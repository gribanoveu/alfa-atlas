import { useEffect, useMemo, useState } from "react";
import { Check, Trash2, X } from "lucide-react";
import { useArtifacts } from "../../hooks/useArtifacts";
import { ARTIFACT_KIND_LABELS, type ArtifactSummary } from "../../lib/artifacts";
import {
  ARTIFACT_KIND_OPTIONS,
  ARTIFACT_SORT_OPTIONS,
  artifactProjectLabel,
  artifactProjectOptions,
  filterAndSortArtifacts,
  type ArtifactSortKey,
} from "../../lib/artifactFilters";
import { LogSelect } from "../ToolLog/LogSelect";
import "../ToolLog/ToolCallLogModal.css";
import "./ArtifactsModal.css";

type ArtifactsModalProps = {
  onClose: () => void;
  /** Opens the artifact's builder tab and closes this dialog. */
  onOpenArtifact: (artifactId: string) => void;
};

function formatShortDate(ms: number): string {
  return new Date(ms).toLocaleString("ru-RU", {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function statusLabel(artifact: ArtifactSummary): string {
  return artifact.status === "ready" ? "готов" : "черновик";
}

/** Every saved artifact, from every project.
 *
 *  A table with a filter bar rather than a list of cards, and deliberately
 *  the tool-call log's dialog: this stopped being "the artifacts of the repo
 *  you have open" — an artifact is filed under the project it was written
 *  in, but one ticket routinely spans several services — and a cross-project
 *  list is something you narrow, not something you scroll. */
export function ArtifactsModal({ onClose, onOpenArtifact }: ArtifactsModalProps) {
  const { artifacts, loading, error, remove } = useArtifacts(true);
  const [search, setSearch] = useState("");
  const [project, setProject] = useState("");
  const [kind, setKind] = useState("");
  const [sort, setSort] = useState<ArtifactSortKey>("recent");

  // Which row's delete button is showing its confirmation. Reset whenever
  // the list changes so a confirmation armed on one row cannot survive into
  // a different row taking its place.
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  useEffect(() => {
    setConfirmingId(null);
  }, [artifacts]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  // Built from what is actually stored, not from the projects that happen to
  // be configured: a project the user no longer opens still owns artifacts.
  const projectOptions = useMemo(() => artifactProjectOptions(artifacts), [artifacts]);
  const shown = useMemo(
    () => filterAndSortArtifacts(artifacts, { search, project, kind }, sort),
    [artifacts, search, project, kind, sort],
  );

  const filtered = shown.length !== artifacts.length;

  return (
    <div className="tool-log-backdrop" role="presentation" onClick={onClose}>
      <div
        className="tool-log-dialog artifacts-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="artifacts-dialog-title"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="tool-log-header">
          <h2 className="tool-log-title" id="artifacts-dialog-title">
            Артефакты
          </h2>
          <div className="tool-log-header-actions">
            {/* Creating a new artifact lives in the Утилиты panel (one
                card per `ARTIFACT_KINDS` entry) — this dialog is
                browse/open/delete only, not a second entry point for it. */}
            <button type="button" className="tool-log-close" onClick={onClose} aria-label="Закрыть">
              ×
            </button>
          </div>
        </header>

        <div className="tool-log-filters">
          <input
            type="text"
            className="tool-log-search"
            placeholder="Поиск по названию, содержимому или проекту…"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
          />
          <LogSelect
            label="Проект"
            value={project}
            options={projectOptions}
            onChange={setProject}
            className="artifacts-project-select"
          />
          <LogSelect label="Тип" value={kind} options={ARTIFACT_KIND_OPTIONS} onChange={setKind} />
          <LogSelect
            label="Сортировка"
            value={sort}
            options={ARTIFACT_SORT_OPTIONS}
            onChange={(value) => setSort(value as ArtifactSortKey)}
          />
        </div>

        {error ? <div className="tool-log-error">{error}</div> : null}

        <div className="tool-log-body">
          <table className="tool-log-table artifacts-table">
            <thead>
              <tr>
                <th>Название</th>
                <th>Что внутри</th>
                <th>Тип</th>
                <th>Проект</th>
                <th>Статус</th>
                <th>Обновлён</th>
                <th aria-label="Действия" />
              </tr>
            </thead>
            <tbody>
              {shown.map((artifact) => (
                <tr
                  key={artifact.id}
                  className="tool-log-row"
                  onClick={() => {
                    onOpenArtifact(artifact.id);
                    onClose();
                  }}
                >
                  <td className="artifacts-cell-title" title={artifact.title}>
                    {artifact.title}
                  </td>
                  <td className="artifacts-cell-subtitle" title={artifact.subtitle}>
                    {artifact.subtitle || "—"}
                  </td>
                  <td>{ARTIFACT_KIND_LABELS[artifact.kind]}</td>
                  <td
                    className="artifacts-cell-project"
                    title={artifact.repoRoot ?? undefined}
                  >
                    {artifactProjectLabel(artifact)}
                  </td>
                  <td>
                    <span className={`artifacts-status artifacts-status-${artifact.status}`}>
                      <span className="artifacts-status-dot" aria-hidden />
                      {statusLabel(artifact)}
                    </span>
                  </td>
                  <td>{formatShortDate(artifact.updatedAtMs)}</td>
                  {/* The row opens the artifact, so everything in this cell
                      has to stop the click from reaching it — deleting and
                      opening are one pixel apart. */}
                  <td
                    className="artifacts-cell-actions"
                    onClick={(event) => event.stopPropagation()}
                  >
                    {confirmingId === artifact.id ? (
                      <span className="artifacts-confirm">
                        <span className="artifacts-confirm-text">Удалить?</span>
                        <button
                          type="button"
                          className="artifacts-icon-btn artifacts-icon-btn-danger"
                          aria-label="Подтвердить удаление"
                          onClick={() => void remove(artifact.id)}
                        >
                          <Check size={13} strokeWidth={2.25} aria-hidden />
                        </button>
                        <button
                          type="button"
                          className="artifacts-icon-btn"
                          aria-label="Отменить удаление"
                          onClick={() => setConfirmingId(null)}
                        >
                          <X size={13} strokeWidth={2.25} aria-hidden />
                        </button>
                      </span>
                    ) : (
                      <button
                        type="button"
                        className="artifacts-icon-btn"
                        aria-label="Удалить артефакт"
                        onClick={() => setConfirmingId(artifact.id)}
                      >
                        <Trash2 size={13} strokeWidth={1.75} aria-hidden />
                      </button>
                    )}
                  </td>
                </tr>
              ))}
              {!loading && shown.length === 0 ? (
                <tr>
                  <td colSpan={7} className="tool-log-empty">
                    {artifacts.length === 0
                      ? "Пока нет сохранённых артефактов. Ассистент попросит собрать один, когда ему не хватит данных о запросе — или начните сами."
                      : "Ничего не найдено"}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>

        <footer className="tool-log-footer">
          <span className="tool-log-range">
            {loading
              ? "Загрузка…"
              : filtered
                ? `Показано: ${shown.length} из ${artifacts.length}`
                : `Артефактов: ${artifacts.length}`}
          </span>
        </footer>
      </div>
    </div>
  );
}
