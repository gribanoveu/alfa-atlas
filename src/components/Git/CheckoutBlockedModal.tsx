import "../Welcome/CloneRepoModal.css";

type CheckoutBlockedModalProps = {
  branchName: string;
  mode: "checkout" | "create";
  busy: boolean;
  onCancel: () => void;
  onOpenCommit: () => void;
  onDiscardAndContinue: () => void;
};

export function CheckoutBlockedModal({
  branchName,
  mode,
  busy,
  onCancel,
  onOpenCommit,
  onDiscardAndContinue,
}: CheckoutBlockedModalProps) {
  const actionLabel =
    mode === "create" ? "Создать ветку" : "Переключить ветку";

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
          {mode === "create" ? (
            <>
              Перед созданием ветки <strong>{branchName}</strong> нужно
              разобраться с текущими файлами добавленными в отслеживаемые, 
              но для которых не сделан коммит.
            </>
          ) : (
            <>
              Перед переключением на <strong>{branchName}</strong> нужно
              разобраться с текущими файлами добавленными в отслеживаемые, 
              но для которых не сделан коммит.
            </>
          )}
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
           сохраненному состоянию, а затем выполнит: {actionLabel.toLowerCase()}.
        </p>
       
        <div className="clone-modal-actions">
          <button
            type="button"
            className="clone-modal-btn"
            disabled={busy}
            onClick={onDiscardAndContinue}
          >
            {mode === "create"
              ? "Отменить изменения и создать"
              : "Отменить изменения и переключить"}
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
