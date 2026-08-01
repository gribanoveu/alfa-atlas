import { Check } from "lucide-react";
import type { OnboardingCardView } from "../../hooks/useOnboarding";
import "./OnboardingCard.css";

type OnboardingCardProps = {
  card: OnboardingCardView;
  onOpen: (id: string) => void;
  onDismiss: (id: string) => void;
};

export function OnboardingCard({ card, onOpen, onDismiss }: OnboardingCardProps) {
  const Icon = card.icon;

  return (
    <div
      className={`onboarding-card ${card.completed ? "is-completed" : ""}`}
      role="button"
      tabIndex={0}
      onClick={() => onOpen(card.id)}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") onOpen(card.id);
      }}
    >
      <div className="onboarding-card-icon">
        <Icon size={14} strokeWidth={1.75} aria-hidden />
      </div>
      <div className="onboarding-card-body">
        <span className="onboarding-card-title">{card.title}</span>
        <span className="onboarding-card-desc">{card.description}</span>
        {card.completed ? (
          <span className="onboarding-card-done">
            <Check size={11} strokeWidth={2} aria-hidden />
            Пройдено
          </span>
        ) : (
          <div className="onboarding-card-actions">
            <button
              type="button"
              className="onboarding-card-btn primary"
              onClick={(event) => {
                event.stopPropagation();
                onOpen(card.id);
              }}
            >
              Начать
            </button>
            <button
              type="button"
              className="onboarding-card-btn"
              onClick={(event) => {
                event.stopPropagation();
                onDismiss(card.id);
              }}
            >
              Не интересует
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
