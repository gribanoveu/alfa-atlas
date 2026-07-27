import { useState } from "react";
import type { PullMode } from "../../lib/git";
import "../Welcome/CloneRepoModal.css";
import "./PullUpdateModal.css";

type PullUpdateModalProps = {
  busy: boolean;
  onCancel: () => void;
  onConfirm: (mode: PullMode) => void;
  onRequestResetToRemote: () => void;
};

export function PullUpdateModal({
  busy,
  onCancel,
  onConfirm,
  onRequestResetToRemote,
}: PullUpdateModalProps) {
  const [mode, setMode] = useState<PullMode>("merge");

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (!busy && event.target === event.currentTarget) onCancel();
      }}
    >
      <div
        className="clone-modal pull-update-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="pull-update-title"
      >
        <div className="clone-modal-title" id="pull-update-title">
          Обновить проект
        </div>
        <div className="clone-modal-message">
          Как объединить изменения с сервера с текущей веткой?
        </div>

        <fieldset className="pull-update-options" disabled={busy}>
          <label className="pull-update-option">
            <input
              type="radio"
              name="pull-mode"
              checked={mode === "merge"}
              onChange={() => setMode("merge")}
            />
            <span>
              Объединить изменения с сервера с вашими (merge)
            </span>
          </label>
          <label className="pull-update-option">
            <input
              type="radio"
              name="pull-mode"
              checked={mode === "rebase"}
              onChange={() => setMode("rebase")}
            />
            <span>Переложить ваши коммиты поверх сервера (rebase)</span>
          </label>
        </fieldset>

        <p className="pull-update-hint">
          Если не уверены, выберите merge — это рекомендуемый вариант.
        </p>

        <div className="clone-modal-actions pull-update-actions">
          <button
            type="button"
            className="clone-modal-btn"
            disabled={busy}
            onClick={onRequestResetToRemote}
            title="Локальные коммиты будут удалены"
          >
            Сбросить к версии на сервере…
          </button>
          <div className="pull-update-actions-end">
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
              onClick={() => onConfirm(mode)}
            >
              Обновить
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
