import { useEffect, useState } from "react";
import type { GitCommitSummary, GitFileStatus } from "../../lib/git";
import "../Welcome/CloneRepoModal.css";
import "./GitCommitPreviewModal.css";

const STATUS_LABELS: Record<string, string> = {
  M: "Изменён",
  A: "Добавлен",
  D: "Удалён",
  R: "Переименован",
};

type GitCommitPreviewModalProps = {
  commit: GitCommitSummary;
  onClose: () => void;
  onLoadFiles: (commitHash: string) => Promise<GitFileStatus[] | null>;
  onOpenFile: (commitHash: string, file: GitFileStatus) => void;
};

export function GitCommitPreviewModal({
  commit,
  onClose,
  onLoadFiles,
  onOpenFile,
}: GitCommitPreviewModalProps) {
  const [files, setFiles] = useState<GitFileStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void onLoadFiles(commit.hash).then((result) => {
      if (cancelled) return;
      if (!result) {
        setError("Не удалось загрузить список файлов");
      } else {
        setFiles(result);
      }
      setLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, [commit.hash, onLoadFiles]);

  return (
    <div
      className="clone-modal-backdrop git-commit-preview-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="clone-modal git-commit-preview-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="commit-preview-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="commit-preview-title">
          Коммит {commit.hash}
        </div>
        <div className="clone-modal-message">{commit.message}</div>
        <div className="git-commit-preview-body">
          {loading ? (
            <div className="clone-modal-message">Загрузка файлов…</div>
          ) : error ? (
            <div className="clone-modal-message">{error}</div>
          ) : files.length === 0 ? (
            <div className="clone-modal-message">Нет изменённых файлов</div>
          ) : (
            <ul className="git-commit-preview-files">
              {files.map((file) => (
                <li key={file.path}>
                  <button
                    type="button"
                    className="git-commit-preview-file-btn"
                    onClick={() => onOpenFile(commit.hash, file)}
                    title={file.path}
                  >
                    <span className="git-commit-preview-file-path">{file.path}</span>
                    <span
                      className={`git-commit-preview-file-status git-commit-preview-status-${file.status}`}
                    >
                      {STATUS_LABELS[file.status] ?? file.status}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
        <div className="clone-modal-actions git-commit-preview-actions">
          <button type="button" className="clone-modal-btn primary" onClick={onClose} autoFocus>
            Закрыть
          </button>
        </div>
      </div>
    </div>
  );
}
