import { useEffect, useRef, useState } from "react";
import "../Welcome/CloneRepoModal.css";
import { toMessage } from "../../lib/errors";
import type { FileTreeDeleteTarget } from "./FileTree";

type DeleteConfirmModalProps = {
  target: FileTreeDeleteTarget;
  onCancel: () => void;
  onConfirm: (target: FileTreeDeleteTarget) => Promise<void>;
};

export function DeleteConfirmModal({
  target,
  onCancel,
  onConfirm,
}: DeleteConfirmModalProps) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const confirmRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    confirmRef.current?.focus();
  }, []);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await onConfirm(target);
    } catch (e) {
      setError(toMessage(e));
      setBusy(false);
    }
  };

  const message = target.isDir
    ? "Папка и всё её содержимое будут удалены безвозвратно."
    : "Файл будет удалён безвозвратно.";

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onClick={onCancel}
    >
      <div
        className="clone-modal"
        role="dialog"
        aria-labelledby="delete-confirm-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="delete-confirm-title">
          Удалить {target.isDir ? "папку" : "файл"}?
        </div>

        <div className="clone-modal-message">
          <div style={{ wordBreak: "break-all" }}>{target.path}</div>
        </div>

        <p
          style={{
            margin: 0,
            fontSize: 12.5,
            color: "var(--text-1)",
            lineHeight: 1.45,
          }}
        >
          {message}
        </p>

        {error ? <div className="clone-modal-message">{error}</div> : null}

        <div className="clone-modal-actions">
          <button type="button" className="clone-modal-btn" onClick={onCancel}>
            Отмена
          </button>
          <button
            ref={confirmRef}
            type="button"
            className="clone-modal-btn primary danger"
            onClick={() => void submit()}
            disabled={busy}
          >
            Удалить
          </button>
        </div>
      </div>
    </div>
  );
}
