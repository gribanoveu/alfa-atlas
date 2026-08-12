import { useState } from "react";
import { Undo2 } from "lucide-react";
import { useOnboarding } from "../../hooks/useOnboarding";
import type { GitActionLogEntry } from "../../lib/gitActionLog";
import { OnboardingCard } from "./OnboardingCard";
import { OnboardingModal } from "./OnboardingModal";
import "./NotificationsPanel.css";

function formatLogTime(unixMillis: number): string {
  const diffMin = Math.round((Date.now() - unixMillis) / 60000);
  if (diffMin < 1) return "только что";
  if (diffMin < 60) return `${diffMin} мин назад`;
  const diffH = Math.round(diffMin / 60);
  if (diffH < 24) return `${diffH} ч назад`;
  const diffD = Math.round(diffH / 24);
  return `${diffD} дн назад`;
}

function GitActionLogRow({
  entry,
  busy,
  onUndo,
}: {
  entry: GitActionLogEntry;
  busy: boolean;
  onUndo: () => void;
}) {
  const showUndo = entry.undoable && !entry.undone;
  return (
    <div className={`git-action-log-row${entry.undone ? " git-action-log-row-undone" : ""}`}>
      <div className="git-action-log-row-main">
        <span className="git-action-log-summary">{entry.summary}</span>
        <span className="git-action-log-time">
          {formatLogTime(entry.createdAt)}
          {entry.undone ? " · отменено" : ""}
        </span>
      </div>
      {showUndo ? (
        <button
          type="button"
          className="git-action-log-undo-btn"
          disabled={busy}
          onClick={onUndo}
          title="Отменить это действие"
          aria-label={`Отменить: ${entry.summary}`}
        >
          <Undo2 size={13} aria-hidden />
          Отменить
        </button>
      ) : null}
    </div>
  );
}

type NotificationsPanelProps = {
  gitActionLog?: {
    entries: GitActionLogEntry[];
    busy: boolean;
    onUndo: (entry: GitActionLogEntry) => void;
  };
};

export function NotificationsPanel({ gitActionLog }: NotificationsPanelProps) {
  const { cards, complete } = useOnboarding();
  const [activeCardId, setActiveCardId] = useState<string | null>(null);
  const activeCard = cards.find((card) => card.id === activeCardId) ?? null;
  const entries = gitActionLog?.entries ?? [];

  return (
    <div className="notifications-panel">
      <section className="notifications-section notifications-section-alerts">
        <h3 className="notifications-section-title">Уведомления</h3>
        {entries.length === 0 ? (
          <div className="panel-empty">Нет активных уведомлений</div>
        ) : (
          <div className="git-action-log-list">
            {entries.map((entry) => (
              <GitActionLogRow
                key={entry.id}
                entry={entry}
                busy={gitActionLog?.busy ?? false}
                onUndo={() => gitActionLog?.onUndo(entry)}
              />
            ))}
          </div>
        )}
      </section>

      <section className="notifications-section notifications-section-onboarding">
        <h3 className="notifications-section-title">Начать работу</h3>
        <div className="onboarding-card-list">
          {cards.map((card) => (
            <OnboardingCard
              key={card.id}
              card={card}
              onOpen={setActiveCardId}
              onDismiss={complete}
            />
          ))}
        </div>
      </section>

      {activeCard ? (
        <OnboardingModal
          card={activeCard}
          onClose={() => setActiveCardId(null)}
          onComplete={complete}
        />
      ) : null}
    </div>
  );
}
