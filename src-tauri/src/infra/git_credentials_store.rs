use std::fs;

use crate::domain::git::GitCredentials;
use crate::domain::settings::SettingsError;

const CREDENTIALS_FILE_NAME: &str = "git_credentials.json";

fn credentials_path() -> Result<std::path::PathBuf, SettingsError> {
    let dir = crate::infra::settings_store::settings_dir()?;
    Ok(dir.join(CREDENTIALS_FILE_NAME))
}

/// Loads `GitCredentials` from `~/.docflow/git_credentials.json`.
/// Missing file yields `GitCredentials::default()`.
pub fn load() -> Result<GitCredentials, SettingsError> {
    let path = credentials_path()?;
    if !path.exists() {
        return Ok(GitCredentials::default());
    }
    let contents = fs::read_to_string(&path).map_err(SettingsError::Read)?;
    let creds = serde_json::from_str(&contents).map_err(SettingsError::Parse)?;
    Ok(creds)
}

pub fn save(credentials: &GitCredentials) -> Result<(), SettingsError> {
    let path = credentials_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(SettingsError::CreateDir)?;
    }
    let contents =
        serde_json::to_string_pretty(credentials).map_err(SettingsError::Serialize)?;
    fs::write(&path, contents).map_err(SettingsError::Write)?;
    Ok(())
}
