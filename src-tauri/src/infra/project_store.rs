use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::project_config::{ProjectConfig, ProjectError};

const PROJECT_DIR_NAME: &str = ".alfa-atlas";
const PROJECT_FILE_NAME: &str = "project.json";

fn resolve_repo_root(repo_root: &str) -> Result<PathBuf, ProjectError> {
    let path = Path::new(repo_root);
    if !path.is_dir() {
        return Err(ProjectError::NotADirectory(path.display().to_string()));
    }
    path.canonicalize().map_err(ProjectError::Canonicalize)
}

fn project_config_path(repo_root: &Path) -> PathBuf {
    repo_root.join(PROJECT_DIR_NAME).join(PROJECT_FILE_NAME)
}

/// Loads `{repo}/.alfa-atlas/project.json`. Missing file → `Ok(None)`.
pub fn load(repo_root: &str) -> Result<Option<ProjectConfig>, ProjectError> {
    let root = resolve_repo_root(repo_root)?;
    let path = project_config_path(&root);
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&path).map_err(ProjectError::Read)?;
    let config: ProjectConfig = serde_json::from_str(&contents).map_err(ProjectError::Parse)?;
    Ok(Some(config))
}

pub fn save(repo_root: &str, config: &ProjectConfig) -> Result<(), ProjectError> {
    let root = resolve_repo_root(repo_root)?;
    let dir = root.join(PROJECT_DIR_NAME);
    fs::create_dir_all(&dir).map_err(ProjectError::CreateDir)?;

    let path = dir.join(PROJECT_FILE_NAME);
    let contents = serde_json::to_string_pretty(config).map_err(ProjectError::Serialize)?;
    fs::write(&path, contents).map_err(ProjectError::Write)?;
    Ok(())
}
