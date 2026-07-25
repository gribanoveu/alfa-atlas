use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::settings::SettingsError;
use crate::domain::workspace::WorkspaceState;

const PROJECT_DIR_NAME: &str = ".docflow";
const WORKSPACE_FILE_NAME: &str = "workspace.json";

fn resolve_project_root(project_root: &str) -> Result<PathBuf, SettingsError> {
    let path = Path::new(project_root);
    if !path.is_dir() {
        return Err(SettingsError::NotADirectory(path.display().to_string()));
    }
    path.canonicalize().map_err(SettingsError::Canonicalize)
}

fn workspace_path(project_root: &Path) -> PathBuf {
    project_root
        .join(PROJECT_DIR_NAME)
        .join(WORKSPACE_FILE_NAME)
}

/// Loads `{project}/.docflow/workspace.json`. Missing file → defaults.
pub fn load(project_root: &str) -> Result<WorkspaceState, SettingsError> {
    let root = resolve_project_root(project_root)?;
    let path = workspace_path(&root);
    if !path.exists() {
        return Ok(WorkspaceState::default());
    }

    let contents = fs::read_to_string(&path).map_err(SettingsError::Read)?;
    let state: WorkspaceState = serde_json::from_str(&contents).map_err(SettingsError::Parse)?;
    Ok(state)
}

pub fn save(project_root: &str, state: &WorkspaceState) -> Result<(), SettingsError> {
    let root = resolve_project_root(project_root)?;
    let dir = root.join(PROJECT_DIR_NAME);
    fs::create_dir_all(&dir).map_err(SettingsError::CreateDir)?;

    let path = dir.join(WORKSPACE_FILE_NAME);
    let contents = serde_json::to_string_pretty(state).map_err(SettingsError::Serialize)?;
    fs::write(&path, contents).map_err(SettingsError::Write)?;
    Ok(())
}
