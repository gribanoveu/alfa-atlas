import { useState } from "react";
import { useOnboarding } from "../../hooks/useOnboarding";
import { OnboardingCard } from "./OnboardingCard";
import { OnboardingModal } from "./OnboardingModal";
import "./NotificationsPanel.css";

export function NotificationsPanel() {
  const { cards, complete } = useOnboarding();
  const [activeCardId, setActiveCardId] = useState<string | null>(null);
  const activeCard = cards.find((card) => card.id === activeCardId) ?? null;

  return (
    <div className="notifications-panel">
      <section className="notifications-section notifications-section-alerts">
        <h3 className="notifications-section-title">Уведомления</h3>
        <div className="panel-empty">Нет активных уведомлений</div>
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
