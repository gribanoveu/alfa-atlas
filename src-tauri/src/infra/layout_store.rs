use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::layout::PanelLayout;
use crate::domain::settings::SettingsError;

const PROJECT_DIR_NAME: &str = ".docflow";
const LAYOUT_FILE_NAME: &str = "layout.json";

fn resolve_project_root(project_root: &str) -> Result<PathBuf, SettingsError> {
    let path = Path::new(project_root);
    if !path.is_dir() {
        return Err(SettingsError::NotADirectory(path.display().to_string()));
    }
    path.canonicalize().map_err(SettingsError::Canonicalize)
}

fn layout_path(project_root: &Path) -> PathBuf {
    project_root.join(PROJECT_DIR_NAME).join(LAYOUT_FILE_NAME)
}

/// Loads panel layout from `{project}/.docflow/layout.json`.
/// Missing file yields defaults.
pub fn load(project_root: &str) -> Result<PanelLayout, SettingsError> {
    let root = resolve_project_root(project_root)?;
    let path = layout_path(&root);
    if !path.exists() {
        return Ok(PanelLayout::default());
    }

    let contents = fs::read_to_string(&path).map_err(SettingsError::Read)?;
    let layout: PanelLayout = serde_json::from_str(&contents).map_err(SettingsError::Parse)?;
    Ok(layout.clamped())
}

pub fn save(project_root: &str, layout: &PanelLayout) -> Result<(), SettingsError> {
    let root = resolve_project_root(project_root)?;
    let dir = root.join(PROJECT_DIR_NAME);
    fs::create_dir_all(&dir).map_err(SettingsError::CreateDir)?;

    let path = dir.join(LAYOUT_FILE_NAME);
    let contents =
        serde_json::to_string_pretty(&layout.clamped()).map_err(SettingsError::Serialize)?;
    fs::write(&path, contents).map_err(SettingsError::Write)?;
    Ok(())
}
