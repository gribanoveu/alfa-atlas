use std::fs;
use std::path::PathBuf;

use crate::domain::settings::{AppSettings, SettingsError};

const SETTINGS_DIR_NAME: &str = ".alfa-atlas";
const SETTINGS_FILE_NAME: &str = "settings.json";

pub fn settings_dir() -> Result<PathBuf, SettingsError> {
    let home = dirs::home_dir().ok_or(SettingsError::HomeDirUnavailable)?;
    Ok(home.join(SETTINGS_DIR_NAME))
}

pub fn settings_path() -> Result<PathBuf, SettingsError> {
    Ok(settings_dir()?.join(SETTINGS_FILE_NAME))
}

/// Loads settings from `~/.alfa-atlas/settings.json`.
/// Missing file yields `AppSettings::default()`.
pub fn load() -> Result<AppSettings, SettingsError> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(AppSettings::default());
    }

    let contents = fs::read_to_string(&path).map_err(SettingsError::Read)?;
    let settings = serde_json::from_str(&contents).map_err(SettingsError::Parse)?;
    Ok(settings)
}

pub fn save(settings: &AppSettings) -> Result<(), SettingsError> {
    let dir = settings_dir()?;
    fs::create_dir_all(&dir).map_err(SettingsError::CreateDir)?;

    let path = dir.join(SETTINGS_FILE_NAME);
    let contents = serde_json::to_string_pretty(settings).map_err(SettingsError::Serialize)?;
    fs::write(&path, contents).map_err(SettingsError::Write)?;
    Ok(())
}
