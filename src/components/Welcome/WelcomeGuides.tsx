import { useState } from "react";
import { Cpu, KeyRound, Play, Settings2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { ONBOARDING_CARDS, type OnboardingCardDef } from "../../data/onboardingCards";
import { useOnboarding } from "../../hooks/useOnboarding";
import { OnboardingModal } from "../RightDock/OnboardingModal";
import "./WelcomeGuides.css";

/** Ролики берутся из того же набора, что и карточки в панели уведомлений,
 * чтобы не держать два списка видео. Ролика про ключ LLM ещё нет — добавьте
 * карточку `setup-llm-key` в `onboardingCards.ts`, и кнопка «Видео»
 * включится сама. */
const GIT_CARD = ONBOARDING_CARDS.find((card) => card.id === "setup-git-ssh") ?? null;
const LLM_CARD = ONBOARDING_CARDS.find((card) => card.id === "setup-llm-key") ?? null;

type Guide = {
  id: string;
  icon: LucideIcon;
  title: string;
  description: string;
  video: OnboardingCardDef | null;
  onOpenSettings: () => void;
};

type WelcomeGuidesProps = {
  onOpenGitKey: () => void;
  onOpenLlmKey: () => void;
};

export function WelcomeGuides({ onOpenGitKey, onOpenLlmKey }: WelcomeGuidesProps) {
  const { complete } = useOnboarding();
  const [videoCard, setVideoCard] = useState<OnboardingCardDef | null>(null);

  const guides: Guide[] = [
    {
      id: "git-key",
      icon: KeyRound,
      title: "Настройте работу с Git",
      description: "SSH-ключ для клонирования и отправки изменений.",
      video: GIT_CARD,
      onOpenSettings: onOpenGitKey,
    },
    {
      id: "llm-key",
      icon: Cpu,
      title: "Добавьте ключ для LLM",
      description: "API-ключ провайдера включает ассистента.",
      video: LLM_CARD,
      onOpenSettings: onOpenLlmKey,
    },
  ];

  return (
    <>
      <ul className="welcome-guides">
        {guides.map((guide) => {
          const Icon = guide.icon;
          return (
            <li key={guide.id} className="welcome-guide">
              <span className="welcome-guide-title">
                <Icon className="welcome-guide-icon" size={13} aria-hidden />
                {guide.title}
              </span>
              <p className="welcome-guide-desc">{guide.description}</p>
              <div className="welcome-guide-actions">
                <button
                  type="button"
                  className="welcome-guide-btn"
                  disabled={!guide.video}
                  title={guide.video ? undefined : "Ролик появится позже"}
                  onClick={() => setVideoCard(guide.video)}
                >
                  <Play size={12} strokeWidth={1.75} aria-hidden />
                  Видео
                </button>
                <button
                  type="button"
                  className="welcome-guide-btn"
                  onClick={guide.onOpenSettings}
                >
                  <Settings2 size={12} strokeWidth={1.75} aria-hidden />
                  Настроить
                </button>
              </div>
            </li>
          );
        })}
      </ul>

      {videoCard ? (
        <OnboardingModal
          card={videoCard}
          onClose={() => setVideoCard(null)}
          onComplete={complete}
        />
      ) : null}
    </>
  );
}
