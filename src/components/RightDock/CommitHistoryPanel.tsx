import { RefreshCw } from "lucide-react";
import type { GitCommitSummary } from "../../lib/git";
import "./GitPanel.css";

type CommitHistoryPanelProps = {
  commits: GitCommitSummary[];
  busy: boolean;
  error: string | null;
  onRefresh: () => void;
};

function formatCommitTime(unixSeconds: number): string {
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

export function CommitHistoryPanel({
  commits,
  busy,
  error,
  onRefresh,
}: CommitHistoryPanelProps) {
  return (
    <div className="git-panel">
      <div className="git-panel-toolbar">
        <button
          type="button"
          className="git-icon-btn"
          title="Обновить список"
          aria-label="Обновить список"
          disabled={busy}
          onClick={onRefresh}
        >
          <RefreshCw size={14} aria-hidden />
        </button>
      </div>

      <div className="git-panel-scroll">
        {commits.length === 0 ? (
          <div className="git-empty" style={{ paddingLeft: 8 }}>
            Записей в истории пока нет
          </div>
        ) : (
          <ul className="git-commit-list">
            {commits.map((item) => (
              <li
                key={item.hash + String(item.time)}
                className="git-commit-row git-commit-row-flat"
              >
                <div className="git-commit-line">
                  <span className="git-commit-hash">{item.hash}</span>
                  <span className="git-commit-msg">{item.message}</span>
                </div>
                <div className="git-commit-meta">
                  {item.author}
                  {item.author ? " · " : null}
                  {formatCommitTime(item.time)}
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>

      {error ? <div className="git-panel-error git-panel-error-dock">{error}</div> : null}
    </div>
  );
}
