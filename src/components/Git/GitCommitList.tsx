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
};

export function GitCommitList({
  commits,
  loading = false,
  emptyMessage = "Коммитов нет",
  overflowCount = 0,
  selectedHash = null,
  onSelect,
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
          const rowClass = `git-commit-row git-commit-row-flat${
            selected ? " git-commit-row-selected" : ""
          }`;

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
              </li>
            );
          }

          return (
            <li key={item.hash + String(item.time)} className={rowClass}>
              <div className="git-commit-row-static">
                <CommitRowContent item={item} />
              </div>
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
