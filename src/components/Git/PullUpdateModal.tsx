import { useState } from "react";
import type { PullMode } from "../../lib/git";
import "../Welcome/CloneRepoModal.css";
import "./PullUpdateModal.css";

type PullUpdateModalProps = {
  busy: boolean;
  onCancel: () => void;
  onConfirm: (mode: PullMode) => void;
  onResetToRemote: () => void;
};

export function PullUpdateModal({
  busy,
  onCancel,
  onConfirm,
  onResetToRemote,
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
          Update Project
        </div>
        <div className="clone-modal-message">
          How should incoming changes be integrated into the current branch?
        </div>

        <fieldset className="pull-update-options" disabled={busy}>
          <label className="pull-update-option">
            <input
              type="radio"
              name="pull-mode"
              checked={mode === "merge"}
              onChange={() => setMode("merge")}
            />
            <span>Merge incoming changes into the current branch</span>
          </label>
          <label className="pull-update-option">
            <input
              type="radio"
              name="pull-mode"
              checked={mode === "rebase"}
              onChange={() => setMode("rebase")}
            />
            <span>Rebase the current branch on top of incoming changes</span>
          </label>
        </fieldset>

        <div className="clone-modal-actions pull-update-actions">
          <button
            type="button"
            className="clone-modal-btn"
            disabled={busy}
            onClick={onResetToRemote}
            title="Discard local commits and match the remote branch"
          >
            Reset to the Remote Branch
          </button>
          <div className="pull-update-actions-end">
            <button
              type="button"
              className="clone-modal-btn"
              disabled={busy}
              onClick={onCancel}
            >
              Cancel
            </button>
            <button
              type="button"
              className="clone-modal-btn primary"
              disabled={busy}
              onClick={() => onConfirm(mode)}
            >
              Ok
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
