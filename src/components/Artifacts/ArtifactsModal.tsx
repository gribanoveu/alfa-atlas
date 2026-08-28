import { useEffect, useState } from "react";
import { Check, Trash2, X } from "lucide-react";
import { useArtifacts } from "../../hooks/useArtifacts";
import { ARTIFACT_KIND_LABELS, type ArtifactSummary } from "../../lib/artifacts";
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

export function ArtifactsModal({ onClose, onOpenArtifact }: ArtifactsModalProps) {
  const { artifacts, loading, error, remove } = useArtifacts(true);
  // Which row's delete button is showing its confirmation. Reset whenever
  // the list changes so a confirmation armed on one row cannot survive into
  // a different row taking its place.
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  useEffect(() => {
    setConfirmingId(null);
  }, [artifacts]);

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

        {error ? <div className="tool-log-error">{error}</div> : null}

        <div className="artifacts-body">
          {loading ? (
            <div className="tool-log-empty">Загрузка…</div>
          ) : artifacts.length === 0 ? (
            <div className="tool-log-empty">
              Пока нет сохранённых артефактов. Ассистент попросит собрать один, когда ему не хватит
              данных о запросе — или начните сами.
            </div>
          ) : (
            <ul className="artifacts-list">
              {artifacts.map((artifact) => (
                <li key={artifact.id} className="artifacts-list-item">
                  <button
                    type="button"
                    className="artifacts-list-main"
                    onClick={() => {
                      onOpenArtifact(artifact.id);
                      onClose();
                    }}
                  >
                    <span className="artifacts-list-title">{artifact.title}</span>
                    <span className="artifacts-list-meta">
                      {ARTIFACT_KIND_LABELS[artifact.kind]}
                      {artifact.subtitle ? ` · ${artifact.subtitle}` : ""} ·{" "}
                      {formatShortDate(artifact.updatedAtMs)}
                    </span>
                  </button>
                  <span className={`artifacts-status artifacts-status-${artifact.status}`}>
                    <span className="artifacts-status-dot" aria-hidden />
                    {statusLabel(artifact)}
                  </span>
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
                </li>
              ))}
            </ul>
          )}
        </div>

        <footer className="tool-log-footer">
          <span className="tool-log-range">
            {loading ? "Загрузка…" : `Артефактов: ${artifacts.length}`}
          </span>
        </footer>
      </div>
    </div>
  );
}
