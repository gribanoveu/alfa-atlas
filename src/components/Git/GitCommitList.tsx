import { Trash2 } from "lucide-react";
import type { GitCommitSummary } from "../../lib/git";
import "../RightDock/GitPanel.css";
import "./GitCommitList.css";

export function formatCommitTime(unixSeconds: number): string {
  try {
    return new Date(unixSeconds * 1000).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return "";
  }
}

type GitCommitListProps = {
  commits: GitCommitSummary[];
  loading?: boolean;
  emptyMessage?: string;
  overflowCount?: number;
  selectedHash?: string | null;
  onSelect?: (hash: string) => void;
  onOpenCommit?: (hash: string) => void;
  unpushedHashes?: ReadonlySet<string>;
  onDropCommit?: (hash: string) => void;
  dropBusy?: boolean;
};

export function GitCommitList({
  commits,
  loading = false,
  emptyMessage = "Коммитов нет",
  overflowCount = 0,
  selectedHash = null,
  onSelect,
  onOpenCommit,
  unpushedHashes,
  onDropCommit,
  dropBusy = false,
}: GitCommitListProps) {
  if (loading) {
    return <div className="git-commit-list-modal-status">Загрузка списка…</div>;
  }

  if (commits.length === 0) {
    return <div className="git-commit-list-modal-status">{emptyMessage}</div>;
  }

  return (
    <div className="git-commit-list-modal">
      <ul className="git-commit-list">
        {commits.map((item) => {
          const selected = selectedHash === item.hash;
          const canDrop = Boolean(
            onDropCommit && unpushedHashes?.has(item.hash),
          );
          const rowClass = `git-commit-row git-commit-row-flat${
            selected ? " git-commit-row-selected" : ""
          }${canDrop ? " git-commit-row-with-action" : ""}`;

          if (onSelect) {
            return (
              <li key={item.hash + String(item.time)} className={rowClass}>
                <button
                  type="button"
                  className="git-commit-row-btn"
                  onClick={() => onSelect(item.hash)}
                  aria-pressed={selected}
                >
                  <CommitRowContent item={item} />
                </button>
                {canDrop ? (
                  <DropCommitButton
                    hash={item.hash}
                    message={item.message}
                    busy={dropBusy}
                    onDrop={onDropCommit!}
                  />
                ) : null}
              </li>
            );
          }

          if (onOpenCommit) {
            return (
              <li key={item.hash + String(item.time)} className={rowClass}>
                <button
                  type="button"
                  className="git-commit-row-btn git-commit-row-open-btn"
                  onClick={() => onOpenCommit(item.hash)}
                  title="Показать изменения в коммите"
                >
                  <CommitRowContent item={item} />
                </button>
                {canDrop ? (
                  <DropCommitButton
                    hash={item.hash}
                    message={item.message}
                    busy={dropBusy}
                    onDrop={onDropCommit!}
                  />
                ) : null}
              </li>
            );
          }

          return (
            <li
              key={item.hash + String(item.time)}
              className={rowClass}
            >
              <div className="git-commit-row-static">
                <CommitRowContent item={item} />
              </div>
              {canDrop ? (
                <DropCommitButton
                  hash={item.hash}
                  message={item.message}
                  busy={dropBusy}
                  onDrop={onDropCommit!}
                />
              ) : null}
            </li>
          );
        })}
      </ul>
      {overflowCount > 0 ? (
        <div className="git-commit-list-overflow">
          и ещё {overflowCount}…
        </div>
      ) : null}
    </div>
  );
}

function DropCommitButton({
  hash,
  message,
  busy,
  onDrop,
}: {
  hash: string;
  message: string;
  busy: boolean;
  onDrop: (hash: string) => void;
}) {
  return (
    <button
      type="button"
      className="git-commit-drop-btn"
      disabled={busy}
      aria-label={`Удалить коммит ${hash}: ${message}`}
      title="Удалить неотправленный коммит"
      onClick={() => onDrop(hash)}
    >
      <Trash2 size={14} aria-hidden />
    </button>
  );
}

function CommitRowContent({ item }: { item: GitCommitSummary }) {
  return (
    <>
      <div className="git-commit-line">
        <span className="git-commit-hash">{item.hash}</span>
        <span className="git-commit-msg">{item.message}</span>
      </div>
      <div className="git-commit-meta">
        {item.author}
        {item.author ? " · " : null}
        {formatCommitTime(item.time)}
      </div>
    </>
  );
}
