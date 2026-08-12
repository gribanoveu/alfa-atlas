import "../Welcome/CloneRepoModal.css";

type DiscardChangesConfirmModalProps = {
  path: string;
  /** True for an untracked ("?") file — discarding it deletes it outright,
   * there's no committed version to revert to. */
  isUntracked: boolean;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
};

export function DiscardChangesConfirmModal({
  path,
  isUntracked,
  busy,
  onCancel,
  onConfirm,
}: DiscardChangesConfirmModalProps) {
  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (!busy && event.target === event.currentTarget) onCancel();
      }}
    >
      <div
        className="clone-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="discard-changes-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="discard-changes-title">
          {isUntracked ? "Удалить файл без возможности восстановления?" : "Отменить изменения?"}
        </div>
        <div className="clone-modal-message">
          {isUntracked ? (
            <>
              Файл <strong>{path}</strong> — новый, его нет в истории git.
              Отмена изменений не «вернёт» его к предыдущей версии, а удалит
              окончательно.
            </>
          ) : (
            <>
              Все изменения в <strong>{path}</strong> будут отменены, файл
              вернётся к последнему коммиту. Несохранённые изменения будут
              потеряны.
            </>
          )}
        </div>
        <div className="clone-modal-actions">
          <button
            type="button"
            className="clone-modal-btn"
            disabled={busy}
            onClick={onCancel}
          >
            Отмена
          </button>
          <button
            type="button"
            className="clone-modal-btn primary danger"
            disabled={busy}
            onClick={onConfirm}
            autoFocus
          >
            {isUntracked ? "Удалить" : "Отменить изменения"}
          </button>
        </div>
      </div>
    </div>
  );
}
