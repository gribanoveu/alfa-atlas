import { useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useNotificationsLayout } from "../../hooks/useNotificationsLayout";
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

function GitActionLogRow({ entry }: { entry: GitActionLogEntry }) {
  return (
    <div className={`git-action-log-row${entry.undone ? " git-action-log-row-undone" : ""}`}>
      <div className="git-action-log-row-main">
        <span className="git-action-log-summary">{entry.summary}</span>
        <span className="git-action-log-time">
          {formatLogTime(entry.createdAt)}
          {entry.undone ? " · отменено" : ""}
        </span>
      </div>
    </div>
  );
}

function SectionToggle({
  expanded,
  label,
  controlsId,
  onToggle,
}: {
  expanded: boolean;
  label: string;
  controlsId: string;
  onToggle: () => void;
}) {
  const Chevron = expanded ? ChevronDown : ChevronRight;
  return (
    <button
      type="button"
      className="notifications-section-toggle"
      aria-expanded={expanded}
      aria-controls={controlsId}
      onClick={onToggle}
    >
      <Chevron className="notifications-section-chevron" size={14} aria-hidden />
      <span className="notifications-section-title">{label}</span>
    </button>
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
  const { alertsExpanded, onboardingExpanded, toggleAlerts, toggleOnboarding } =
    useNotificationsLayout();
  const [activeCardId, setActiveCardId] = useState<string | null>(null);
  const activeCard = cards.find((card) => card.id === activeCardId) ?? null;
  const entries = gitActionLog?.entries ?? [];

  return (
    <div
      className={
        "notifications-panel" +
        (alertsExpanded ? "" : " alerts-collapsed") +
        (onboardingExpanded ? "" : " onboarding-collapsed")
      }
    >
      <section className="notifications-section notifications-section-alerts">
        <SectionToggle
          expanded={alertsExpanded}
          label="Уведомления"
          controlsId="notifications-alerts-body"
          onToggle={toggleAlerts}
        />
        {alertsExpanded ? (
          <div className="notifications-section-body" id="notifications-alerts-body">
            {entries.length === 0 ? (
              <div className="panel-empty">Нет активных уведомлений</div>
            ) : (
              <div className="git-action-log-list">
                {entries.map((entry) => (
                  <GitActionLogRow key={entry.id} entry={entry} />
                ))}
              </div>
            )}
          </div>
        ) : null}
      </section>

      <section className="notifications-section notifications-section-onboarding">
        <SectionToggle
          expanded={onboardingExpanded}
          label="Начать работу"
          controlsId="notifications-onboarding-body"
          onToggle={toggleOnboarding}
        />
        {onboardingExpanded ? (
          <div className="notifications-section-body" id="notifications-onboarding-body">
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
          </div>
        ) : null}
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
