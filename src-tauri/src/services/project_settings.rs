use std::path::{Path, PathBuf};

use crate::domain::settings::{ProjectSettings, SettingsError};
use crate::infra::settings_store;

/// Returns a canonical project root if it is saved and still exists as a directory.
/// Clears a stale path from settings when the directory is gone.
pub fn load_project_root() -> Result<Option<String>, SettingsError> {
    let mut settings = settings_store::load()?;
    let Some(root) = settings.project.root.clone() else {
        return Ok(None);
    };

    let path = PathBuf::from(&root);
    if path.is_dir() {
        let canonical = path.canonicalize().map_err(SettingsError::Canonicalize)?;
        return Ok(Some(canonical.to_string_lossy().into_owned()));
    }

    settings.project.root = None;
    settings_store::save(&settings)?;
    Ok(None)
}

pub fn set_project_root(path: &str) -> Result<String, SettingsError> {
    let path = Path::new(path);
    if !path.is_dir() {
        return Err(SettingsError::NotADirectory(path.display().to_string()));
    }

    let canonical = path.canonicalize().map_err(SettingsError::Canonicalize)?;
    let root = canonical.to_string_lossy().into_owned();

    let mut settings = settings_store::load().unwrap_or_default();
    settings.project = ProjectSettings {
        root: Some(root.clone()),
    };
    settings_store::save(&settings)?;
    Ok(root)
}

pub fn clear_project_root() -> Result<(), SettingsError> {
    let mut settings = settings_store::load().unwrap_or_default();
    settings.project.root = None;
    settings_store::save(&settings)
}
