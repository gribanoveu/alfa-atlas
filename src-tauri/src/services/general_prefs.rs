use crate::domain::settings::{GeneralPrefs, SettingsError};
use crate::infra::settings_store;

pub fn load_general_prefs() -> Result<GeneralPrefs, SettingsError> {
    Ok(settings_store::load()?.general)
}

pub fn save_general_prefs(prefs: GeneralPrefs) -> Result<(), SettingsError> {
    let mut settings = settings_store::load().unwrap_or_default();
    settings.general = prefs;
    settings_store::save(&settings)
}
