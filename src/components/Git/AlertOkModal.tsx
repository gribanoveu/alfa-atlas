import "../Welcome/CloneRepoModal.css";
import "./AlertOkModal.css";

type AlertOkModalProps = {
  title?: string;
  message: string;
  variant?: "error" | "info";
  onClose: () => void;
};

export function AlertOkModal({
  title,
  message,
  variant = "error",
  onClose,
}: AlertOkModalProps) {
  const resolvedTitle = title ?? (variant === "info" ? "Готово" : "Ошибка");
  const confirmLabel = variant === "info" ? "Понятно" : "Ok";

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className={`clone-modal${variant === "info" ? " alert-info" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="alert-ok-title"
      >
        <div className="clone-modal-title" id="alert-ok-title">
          {resolvedTitle}
        </div>
        <div className="clone-modal-message">{message}</div>
        <div className="clone-modal-actions">
          <button
            type="button"
            className="clone-modal-btn primary"
            onClick={onClose}
            autoFocus
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
