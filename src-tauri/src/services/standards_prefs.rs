use crate::domain::settings::SettingsError;
use crate::domain::standards::StandardsRuleConfig;
use crate::infra::settings_store;

pub fn load_standards_config() -> Result<StandardsRuleConfig, SettingsError> {
    Ok(settings_store::load()?.standards)
}

pub fn save_standards_config(config: StandardsRuleConfig) -> Result<(), SettingsError> {
    let mut settings = settings_store::load().unwrap_or_default();
    settings.standards = config;
    settings_store::save(&settings)
}
