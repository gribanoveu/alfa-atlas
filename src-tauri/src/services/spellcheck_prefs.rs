use crate::domain::settings::SettingsError;
use crate::domain::spellcheck::SpellcheckConfig;
use crate::infra::settings_store;

pub fn load_spellcheck_config() -> Result<SpellcheckConfig, SettingsError> {
    Ok(settings_store::load()?.spellcheck)
}

pub fn save_spellcheck_config(config: SpellcheckConfig) -> Result<(), SettingsError> {
    let mut settings = settings_store::load().unwrap_or_default();
    settings.spellcheck = config;
    settings_store::save(&settings)
}
