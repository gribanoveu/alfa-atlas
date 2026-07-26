import "../Welcome/CloneRepoModal.css";

type ResetRemoteConfirmModalProps = {
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
};

export function ResetRemoteConfirmModal({
  busy,
  onCancel,
  onConfirm,
}: ResetRemoteConfirmModalProps) {
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
        aria-labelledby="reset-remote-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="reset-remote-title">
          Сбросить ветку к серверу?
        </div>
        <div className="clone-modal-message">
          Локальные коммиты и незакоммиченные изменения будут безвозвратно
          потеряны. Ветка станет такой же, как на удалённом сервере.
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
            Сбросить
          </button>
        </div>
      </div>
    </div>
  );
}
