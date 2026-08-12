import "../Welcome/CloneRepoModal.css";

// Only fires for branch creation — checking out an existing branch
// auto-stashes uncommitted changes instead of blocking (see
// App.tsx's performCheckout/handleCheckoutOutcome), since there's a
// destination branch to restore them onto. Creating a branch has no
// separate destination tree, so the auto-stash flow doesn't apply here.
type CheckoutBlockedModalProps = {
  branchName: string;
  mode: "create";
  busy: boolean;
  onCancel: () => void;
  onOpenCommit: () => void;
  onDiscardAndContinue: () => void;
};

export function CheckoutBlockedModal({
  branchName,
  busy,
  onCancel,
  onOpenCommit,
  onDiscardAndContinue,
}: CheckoutBlockedModalProps) {
  const actionLabel = "Создать ветку";

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
        aria-labelledby="checkout-blocked-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="checkout-blocked-title">
          В коммит добавлены файлы, но коммит не сделан
        </div>
        <div className="clone-modal-message">
          Перед созданием ветки <strong>{branchName}</strong> нужно
          разобраться с текущими файлами добавленными в Stage,
          но для которых не сделан коммит.
        </div>
        <p
          style={{
            margin: 0,
            fontSize: 12.5,
            color: "var(--text-1)",
            lineHeight: 1.45,
          }}
        >
          Закоммитьте изменения в панели «Commit / Коммит» или уберите их из добавленных (список Stage).
          Для этого нажмите на минус справа от файла, удалять файлы не нужно.
          Новые файлы, которые не добавлены в Stage, не мешают переключению.
        </p>
        <p
          style={{
            margin: 0,
            fontSize: 12.5,
            color: "var(--text-1)",
            lineHeight: 1.45,
          }}
        >
           «Отменить изменения» сбросит отслеживаемые файлы к последнему
           сохраненному состоянию (изменения будут потеряны), а затем выполнит: {actionLabel.toLowerCase()}.
        </p>
       
        <div className="clone-modal-actions">
          <button
            type="button"
            className="clone-modal-btn"
            disabled={busy}
            onClick={onDiscardAndContinue}
          >
            Отменить изменения и создать
          </button>
          <button
            type="button"
            className="clone-modal-btn"
            disabled={busy}
            onClick={onOpenCommit}
          >
            К панели Commit
          </button>
          <button
            type="button"
            className="clone-modal-btn primary danger"
            disabled={busy}
            onClick={onCancel}
            autoFocus
          >
            Отмена
          </button>
        </div>
      </div>
    </div>
  );
}
