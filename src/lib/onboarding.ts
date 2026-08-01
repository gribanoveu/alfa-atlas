import { invoke } from "@tauri-apps/api/core";

export type OnboardingState = {
  completed: string[];
};

export function getOnboardingState(): Promise<OnboardingState> {
  return invoke<OnboardingState>("get_onboarding_state");
}

export function markOnboardingCompleted(id: string): Promise<OnboardingState> {
  return invoke<OnboardingState>("mark_onboarding_completed", { id });
}
