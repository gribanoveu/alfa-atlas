import "../Welcome/CloneRepoModal.css";
import type { GitBranchInfo } from "../../lib/git";

type DeleteBranchConfirmModalProps = {
  branch: GitBranchInfo;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
};

export function DeleteBranchConfirmModal({
  branch,
  busy,
  onCancel,
  onConfirm,
}: DeleteBranchConfirmModalProps) {
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
        aria-labelledby="delete-branch-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="delete-branch-title">
          Удалить ветку «{branch.name}»?
        </div>
        <div className="clone-modal-message">
          Ветка и её локальная история (если не сохранена в другой ветке или
          на сервере) будут безвозвратно удалены.
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
