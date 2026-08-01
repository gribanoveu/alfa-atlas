use crate::domain::onboarding::OnboardingState;
use crate::infra::onboarding_store;

#[tauri::command]
pub fn get_onboarding_state() -> Result<OnboardingState, String> {
    onboarding_store::load().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mark_onboarding_completed(id: String) -> Result<OnboardingState, String> {
    let mut state = onboarding_store::load().map_err(|e| e.to_string())?;
    if !state.completed.contains(&id) {
        state.completed.push(id);
        onboarding_store::save(&state).map_err(|e| e.to_string())?;
    }
    Ok(state)
}
