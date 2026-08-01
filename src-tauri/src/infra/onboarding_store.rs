use std::fs;

use crate::domain::onboarding::OnboardingState;
use crate::domain::settings::SettingsError;

const ONBOARDING_FILE_NAME: &str = "onboarding.json";

fn onboarding_path() -> Result<std::path::PathBuf, SettingsError> {
    let dir = crate::infra::settings_store::settings_dir()?;
    Ok(dir.join(ONBOARDING_FILE_NAME))
}

/// Loads `OnboardingState` from `~/.atlas/onboarding.json`.
/// Missing file is created with the default (empty) state on first read.
pub fn load() -> Result<OnboardingState, SettingsError> {
    let path = onboarding_path()?;
    if !path.exists() {
        let state = OnboardingState::default();
        save(&state)?;
        return Ok(state);
    }
    let contents = fs::read_to_string(&path).map_err(SettingsError::Read)?;
    let state = serde_json::from_str(&contents).map_err(SettingsError::Parse)?;
    Ok(state)
}

pub fn save(state: &OnboardingState) -> Result<(), SettingsError> {
    let path = onboarding_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(SettingsError::CreateDir)?;
    }
    let contents = serde_json::to_string_pretty(state).map_err(SettingsError::Serialize)?;
    fs::write(&path, contents).map_err(SettingsError::Write)?;
    Ok(())
}
