use crate::domain::settings::{SettingsError, WindowState};
use crate::infra::settings_store;

/// Returns the last saved window state, or defaults on first run /
/// when settings are missing or unreadable.
pub fn load_window_state() -> WindowState {
    match settings_store::load() {
        Ok(settings) => settings.window.clamped(),
        Err(_) => WindowState::default(),
    }
}

pub fn save_window_state(state: WindowState) -> Result<(), SettingsError> {
    let mut settings = settings_store::load().unwrap_or_default();
    settings.window = state.clamped();
    settings_store::save(&settings)
}
