import { useState } from "react";
import type { GitCommitSummary, PullMode } from "../../lib/git";
import { formatGitProgress, useGitProgress } from "../../hooks/useGitProgress";
import { GitCommitList } from "./GitCommitList";
import "../Welcome/CloneRepoModal.css";
import "./PullUpdateModal.css";

type PullUpdateModalProps = {
  behind: number;
  commits: GitCommitSummary[];
  commitsLoading: boolean;
  busy: boolean;
  onCancel: () => void;
  onConfirm: (mode: PullMode) => void;
  onRequestResetToRemote: () => void;
  onOpenCommit: (hash: string) => void;
};

export function PullUpdateModal({
  behind,
  commits,
  commitsLoading,
  busy,
  onCancel,
  onConfirm,
  onRequestResetToRemote,
  onOpenCommit,
}: PullUpdateModalProps) {
  const [mode, setMode] = useState<PullMode>("merge");
  const gitProgress = useGitProgress();
  const progressLabel = busy ? formatGitProgress(gitProgress.event) : null;
  const displayCount = commitsLoading ? behind : Math.max(behind, commits.length);
  const overflowCount =
    displayCount > commits.length ? displayCount - commits.length : 0;

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
          {commitsLoading
            ? "Проверяем изменения на сервере…"
            : displayCount > 0
              ? `С сервера придёт ${displayCount} ${commitWord(displayCount)}.`
              : "Нет новых коммитов на сервере."}
        </div>

        <div className="pull-update-commits-label">Придёт с сервера</div>
        <GitCommitList
          commits={commits}
          loading={commitsLoading}
          emptyMessage={
            commitsLoading ? "Загрузка списка…" : "Нет новых коммитов"
          }
          overflowCount={overflowCount}
          onOpenCommit={onOpenCommit}
        />

        <div className="clone-modal-message pull-update-merge-prompt">
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
              Объединить изменения с сервера с вашими (merge) - рекомендуется
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
              onClick={() => {
                gitProgress.reset();
                onConfirm(mode);
              }}
            >
              {busy ? (progressLabel ? `Обновление… ${progressLabel}` : "Обновление…") : "Обновить"}
            </button>
          </div>
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
