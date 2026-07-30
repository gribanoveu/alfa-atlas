import "../Welcome/CloneRepoModal.css";

type PushConfirmModalProps = {
  branchName: string | null;
  hasUpstream: boolean;
  ahead: number;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
};

export function PushConfirmModal({
  branchName,
  hasUpstream,
  ahead,
  busy,
  onCancel,
  onConfirm,
}: PushConfirmModalProps) {
  const message = hasUpstream
    ? `Будет отправлено ${ahead} ${commitWord(ahead)} из ветки «${branchName}» на сервер.`
    : `Ветка «${branchName}» ещё не отправлялась на сервер. Она будет создана в удалённом репозитории и привязана как upstream.`;

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
        aria-labelledby="push-confirm-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="push-confirm-title">
          Отправить изменения на сервер?
        </div>
        <div className="clone-modal-message">{message}</div>
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
            className="clone-modal-btn primary"
            disabled={busy}
            onClick={onConfirm}
            autoFocus
          >
            Отправить
          </button>
        </div>
      </div>
    </div>
  );
}

function commitWord(count: number): string {
  const mod10 = count % 10;
  const mod100 = count % 100;
  if (mod10 === 1 && mod100 !== 11) return "коммит";
  if ([2, 3, 4].includes(mod10) && ![12, 13, 14].includes(mod100)) {
    return "коммита";
  }
  return "коммитов";
}
