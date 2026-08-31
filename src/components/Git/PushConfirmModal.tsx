import type { GitCommitSummary } from "../../lib/git";
import { formatGitBusyLabel, useGitProgress } from "../../hooks/useGitProgress";
import { GitCommitList } from "./GitCommitList";
import "../Welcome/CloneRepoModal.css";
import "./PushConfirmModal.css";

type PushConfirmModalProps = {
  branchName: string | null;
  hasUpstream: boolean;
  ahead: number;
  commits: GitCommitSummary[];
  commitsLoading: boolean;
  unpushedHashes: ReadonlySet<string>;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
  onDropCommit: (hash: string) => void;
  onMoveToBranch: () => void;
  onDropAllUnpushed: () => void;
  onOpenCommit: (hash: string) => void;
};

export function PushConfirmModal({
  branchName,
  hasUpstream,
  ahead,
  commits,
  commitsLoading,
  unpushedHashes,
  busy,
  onCancel,
  onConfirm,
  onDropCommit,
  onMoveToBranch,
  onDropAllUnpushed,
  onOpenCommit,
}: PushConfirmModalProps) {
  const gitProgress = useGitProgress();
  const busyLabel = busy ? formatGitBusyLabel("Отправка", gitProgress.event) : null;
  const message = hasUpstream
    ? `Будет отправлено ${ahead} ${commitWord(ahead)} из ветки «${branchName}» на сервер.`
    : `Ветка «${branchName}» ещё не отправлялась на сервер. Она будет создана в удалённом репозитории и привязана как upstream.`;
  const overflowCount = hasUpstream && ahead > commits.length ? ahead - commits.length : 0;
  const showCommitList = hasUpstream ? ahead > 0 : commits.length > 0;
  const hasUnpushed = commits.length > 0 || ahead > 0;

  return (
    <div
      className="clone-modal-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (!busy && event.target === event.currentTarget) onCancel();
      }}
    >
      <div
        className="clone-modal push-confirm-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="push-confirm-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="clone-modal-title" id="push-confirm-title">
          Отправить изменения на сервер?
        </div>
        <div className="clone-modal-message">{message}</div>
        {showCommitList ? (
          <GitCommitList
            commits={commits}
            loading={commitsLoading}
            emptyMessage="Коммиты не найдены"
            overflowCount={overflowCount}
            unpushedHashes={unpushedHashes}
            onDropCommit={onDropCommit}
            onOpenCommit={onOpenCommit}
            dropBusy={busy}
          />
        ) : null}
        {hasUnpushed ? (
          <div className="push-confirm-extra-actions">
            <button
              type="button"
              className="push-confirm-extra-btn"
              disabled={busy}
              onClick={onMoveToBranch}
            >
              Перенести на другую ветку…
            </button>
            {hasUpstream ? (
              <button
                type="button"
                className="push-confirm-extra-btn"
                disabled={busy}
                onClick={onDropAllUnpushed}
              >
                Удалить все неотправленные…
              </button>
            ) : null}
          </div>
        ) : null}
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
            onClick={() => {
              gitProgress.reset();
              onConfirm();
            }}
            autoFocus
          >
            {busy ? busyLabel : "Отправить"}
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
