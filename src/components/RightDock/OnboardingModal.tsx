import { useState } from "react";
import { ChevronLeft, ChevronRight, X } from "lucide-react";
import type { OnboardingCardDef } from "../../data/onboardingCards";
import "./OnboardingModal.css";

type OnboardingModalProps = {
  card: OnboardingCardDef;
  onClose: () => void;
  onComplete: (id: string) => void;
};

export function OnboardingModal({ card, onClose, onComplete }: OnboardingModalProps) {
  const [stepIndex, setStepIndex] = useState(0);
  const step = card.steps[stepIndex];
  const isLastStep = stepIndex === card.steps.length - 1;
  const hasMultipleSteps = card.steps.length > 1;

  return (
    <div className="onboarding-modal-backdrop" role="presentation" onClick={onClose}>
      <div
        className="onboarding-modal"
        role="dialog"
        aria-labelledby="onboarding-modal-title"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="onboarding-modal-head">
          <h2 id="onboarding-modal-title">{card.title}</h2>
          <button
            type="button"
            className="onboarding-modal-close"
            onClick={onClose}
            aria-label="Закрыть"
          >
            <X size={16} strokeWidth={1.75} />
          </button>
        </header>

        <div className="onboarding-modal-video">
          <video
            key={step.videoSrc}
            src={step.videoSrc}
            controls
            autoPlay
            muted
            playsInline
          />
        </div>

        <div>
          <div className="onboarding-modal-text-label">Описание</div>
          <div className="onboarding-modal-text">
            <p>{step.text}</p>
          </div>
        </div>

        <footer className="onboarding-modal-footer">
          {hasMultipleSteps ? (
            <div className="onboarding-modal-dots">
              {card.steps.map((s, index) => (
                <span
                  key={s.videoSrc}
                  className={`onboarding-modal-dot ${index === stepIndex ? "active" : ""}`}
                />
              ))}
            </div>
          ) : (
            <span />
          )}

          <div className="onboarding-modal-nav">
            {hasMultipleSteps && stepIndex > 0 ? (
              <button
                type="button"
                className="onboarding-modal-btn"
                onClick={() => setStepIndex((i) => i - 1)}
              >
                <ChevronLeft size={15} strokeWidth={1.75} />
                Назад
              </button>
            ) : null}

            {isLastStep ? (
              <button
                type="button"
                className="onboarding-modal-btn primary"
                onClick={() => {
                  onComplete(card.id);
                  onClose();
                }}
              >
                Готово
              </button>
            ) : (
              <button
                type="button"
                className="onboarding-modal-btn primary"
                onClick={() => setStepIndex((i) => i + 1)}
              >
                Далее
                <ChevronRight size={15} strokeWidth={1.75} />
              </button>
            )}
          </div>
        </footer>
      </div>
    </div>
  );
}
