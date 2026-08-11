import "../Welcome/CloneRepoModal.css";

type DiscardStashConfirmModalProps = {
  branchName: string;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
};

export function DiscardStashConfirmModal({
  branchName,
  busy,
  onCancel,
  onConfirm,
}: DiscardStashConfirmModalProps) {
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
        aria-labelledby="discard-stash-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="discard-stash-title">
          Удалить отложенные изменения?
        </div>
        <div className="clone-modal-message">
          Отложенные изменения ветки <strong>{branchName}</strong> будут
          удалены без возможности восстановления.
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
            Удалить
          </button>
        </div>
      </div>
    </div>
  );
}
