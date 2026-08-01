use serde::{Deserialize, Serialize};

/// Persisted onboarding progress. Card definitions live in the frontend
/// code; this only tracks which card ids have been completed, in the
/// order they were completed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OnboardingState {
    #[serde(default)]
    pub completed: Vec<String>,
}
