import { useEffect, useState } from "react";
import type { GitFileStatus, GitStashEntry } from "../../lib/git";
import "../Welcome/CloneRepoModal.css";

const STATUS_LABELS: Record<string, string> = {
  M: "Изменён",
  A: "Новый",
  D: "Удалён",
  R: "Переименован",
  "?": "Новый файл",
};

type GitStashPreviewModalProps = {
  entry: GitStashEntry;
  onClose: () => void;
  onLoadFiles: (stashId: string) => Promise<GitFileStatus[] | null>;
  onOpenFile: (file: GitFileStatus) => void;
};

/** Lists the files changed by a shelf entry — reuses the same commit-files
 * API a stash commit is transparently readable through (see
 * `apply_stash_entry`'s doc comment / `commit_files` in git_repo.rs). */
export function GitStashPreviewModal({
  entry,
  onClose,
  onLoadFiles,
  onOpenFile,
}: GitStashPreviewModalProps) {
  const [files, setFiles] = useState<GitFileStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void onLoadFiles(entry.id).then((result) => {
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
  }, [entry.id, onLoadFiles]);

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="clone-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="stash-preview-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="stash-preview-title">
          Отложенные изменения: {entry.branch}
        </div>
        {loading ? (
          <div className="clone-modal-message">Загрузка…</div>
        ) : error ? (
          <div className="clone-modal-message">{error}</div>
        ) : files.length === 0 ? (
          <div className="clone-modal-message">Нет изменённых файлов</div>
        ) : (
          <ul style={{ listStyle: "none", margin: 0, padding: 0, maxHeight: 320, overflowY: "auto" }}>
            {files.map((file) => (
              <li key={file.path}>
                <button
                  type="button"
                  className="clone-modal-btn"
                  style={{ width: "100%", justifyContent: "space-between", display: "flex", marginBottom: 4 }}
                  onClick={() => onOpenFile(file)}
                  title={file.path}
                >
                  <span
                    style={{
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      textAlign: "left",
                    }}
                  >
                    {file.path}
                  </span>
                  <span style={{ flex: "none", marginLeft: 8 }}>
                    {STATUS_LABELS[file.status] ?? file.status}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="clone-modal-actions">
          <button type="button" className="clone-modal-btn primary" onClick={onClose} autoFocus>
            Закрыть
          </button>
        </div>
      </div>
    </div>
  );
}
