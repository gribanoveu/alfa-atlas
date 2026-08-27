import { useState } from "react";
import type { GitCommitSummary, GitResetMode } from "../../lib/git";
import "../Welcome/CloneRepoModal.css";
import "./DropUnpushedConfirmModal.css";

type DropUnpushedConfirmModalProps = {
  commit: GitCommitSummary | null;
  newerCount: number;
  unpushedCount: number;
  busy: boolean;
  onCancel: () => void;
  onConfirm: (mode: GitResetMode) => void;
};

export function DropUnpushedConfirmModal({
  commit,
  newerCount,
  unpushedCount,
  busy,
  onCancel,
  onConfirm,
}: DropUnpushedConfirmModalProps) {
  const [mode, setMode] = useState<GitResetMode>("mixed");
  const dropAll = commit === null;

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (!busy && event.target === event.currentTarget) onCancel();
      }}
    >
      <div
        className="clone-modal drop-unpushed-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="drop-unpushed-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="drop-unpushed-title">
          {dropAll ? "Удалить все неотправленные коммиты?" : "Удалить неотправленный коммит?"}
        </div>
        <div className="clone-modal-message">
          {dropAll ? (
            <>
              Будут удалены все {unpushedCount} неотправленных{" "}
              {commitWord(unpushedCount)} с текущей ветки.
            </>
          ) : (
            <>
              Будет удалён коммит <strong>{commit.hash}</strong> ({commit.message})
              {newerCount > 0
                ? ` и ещё ${newerCount} более новых неотправленных.`
                : "."}
            </>
          )}
        </div>

        <fieldset className="drop-unpushed-options" disabled={busy}>
          <label className="drop-unpushed-option">
            <input
              type="radio"
              name="drop-mode"
              checked={mode === "soft"}
              onChange={() => setMode("soft")}
            />
            <span>Оставить изменения в staged</span>
          </label>
          <label className="drop-unpushed-option">
            <input
              type="radio"
              name="drop-mode"
              checked={mode === "mixed"}
              onChange={() => setMode("mixed")}
            />
            <span>Оставить изменения в файлах (рекомендуется)</span>
          </label>
          <label className="drop-unpushed-option">
            <input
              type="radio"
              name="drop-mode"
              checked={mode === "hard"}
              onChange={() => setMode("hard")}
            />
            <span>Удалить изменения полностью</span>
          </label>
        </fieldset>

        <div className="clone-modal-actions">
          <button type="button" className="clone-modal-btn" disabled={busy} onClick={onCancel}>
            Отмена
          </button>
          <button
            type="button"
            className="clone-modal-btn primary"
            disabled={busy}
            onClick={() => onConfirm(mode)}
          >
            {busy ? "Удаление…" : "Удалить"}
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
