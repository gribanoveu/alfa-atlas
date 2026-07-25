import "../Welcome/CloneRepoModal.css";

type AlertOkModalProps = {
  title?: string;
  message: string;
  onClose: () => void;
};

export function AlertOkModal({
  title = "Ошибка",
  message,
  onClose,
}: AlertOkModalProps) {
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
        aria-labelledby="alert-ok-title"
      >
        <div className="clone-modal-title" id="alert-ok-title">
          {title}
        </div>
        <div className="clone-modal-message">{message}</div>
        <div className="clone-modal-actions">
          <button
            type="button"
            className="clone-modal-btn primary"
            onClick={onClose}
            autoFocus
          >
            Ok
          </button>
        </div>
      </div>
    </div>
  );
}
