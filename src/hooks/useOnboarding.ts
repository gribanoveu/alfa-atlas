import { useCallback, useEffect, useState } from "react";
import { ONBOARDING_CARDS, type OnboardingCardDef } from "../data/onboardingCards";
import { getOnboardingState, markOnboardingCompleted } from "../lib/onboarding";

export type OnboardingCardView = OnboardingCardDef & { completed: boolean };

function orderCards(completed: string[]): OnboardingCardView[] {
  const completedSet = new Set(completed);
  const pending = ONBOARDING_CARDS.filter((card) => !completedSet.has(card.id)).map(
    (card) => ({ ...card, completed: false }),
  );
  const done = completed
    .map((id) => ONBOARDING_CARDS.find((card) => card.id === id))
    .filter((card): card is OnboardingCardDef => Boolean(card))
    .map((card) => ({ ...card, completed: true }));
  return [...pending, ...done];
}

export function useOnboarding() {
  const [completed, setCompleted] = useState<string[]>([]);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let cancelled = false;
    getOnboardingState()
      .then((state) => {
        if (!cancelled) setCompleted(state.completed);
      })
      .finally(() => {
        if (!cancelled) setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const complete = useCallback((id: string) => {
    setCompleted((current) => (current.includes(id) ? current : [...current, id]));
    markOnboardingCompleted(id).catch(() => {
      // Persisted state will resync from disk next launch; keep optimistic UI.
    });
  }, []);

  return {
    loaded,
    cards: orderCards(completed),
    complete,
  };
}
