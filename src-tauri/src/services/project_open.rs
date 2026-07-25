use std::path::{Path, PathBuf};

use crate::domain::paths;
use crate::domain::project_config::{
    OpenedProject, ProbeResult, ProjectConfig, ProjectError,
};
use crate::domain::settings::SettingsError;
use crate::infra::{git_repo, project_store, settings_store};
use crate::services::docs_discovery;

/// Resolve absolute docs root from repo + relative config; validate it exists under repo.
pub fn resolve_docs_root(
    repo_root: &Path,
    docs_root_relative: &str,
) -> Result<PathBuf, ProjectError> {
    let repo_root = repo_root
        .canonicalize()
        .map_err(ProjectError::Canonicalize)?;
    let joined = paths::join_relative(&repo_root, docs_root_relative)?;
    if !joined.is_dir() {
        return Err(ProjectError::NotADirectory(joined.display().to_string()));
    }
    let docs = joined.canonicalize().map_err(ProjectError::Canonicalize)?;
    if !docs.starts_with(&repo_root) {
        return Err(ProjectError::DocsOutsideRepo(docs.display().to_string()));
    }
    Ok(docs)
}

fn load_cached_docs(repo_root: &Path) -> Result<Option<PathBuf>, ProjectError> {
    let root_str = repo_root.to_string_lossy();
    let Some(config) = project_store::load(&root_str)? else {
        return Ok(None);
    };
    match resolve_docs_root(repo_root, &config.docs_root) {
        Ok(docs) => Ok(Some(docs)),
        Err(_) => Ok(None),
    }
}

pub fn probe_open_path(selected_path: &str) -> Result<ProbeResult, ProjectError> {
    let selected = Path::new(selected_path);
    if !selected.is_dir() {
        return Err(ProjectError::NotADirectory(selected_path.to_string()));
    }
    let selected = selected
        .canonicalize()
        .map_err(ProjectError::Canonicalize)?;

    let repo_root = git_repo::discover_repo_root(&selected);

    if let Some(docs) = load_cached_docs(&repo_root)? {
        return Ok(ProbeResult {
            needs_confirm: false,
            root: repo_root.to_string_lossy().into_owned(),
            docs_root: Some(docs.to_string_lossy().into_owned()),
            candidates: vec![],
            suggested_docs_root: Some(docs.to_string_lossy().into_owned()),
        });
    }

    // Prefer scanning from the user-selected folder (may be deeper than repo).
    let scan_root = if selected.starts_with(&repo_root) {
        selected
    } else {
        repo_root.clone()
    };

    let candidates = docs_discovery::find_candidates(&repo_root, &scan_root)?;
    let suggested = candidates.first().map(|c| c.path.clone());

    Ok(ProbeResult {
        needs_confirm: true,
        root: repo_root.to_string_lossy().into_owned(),
        docs_root: None,
        candidates,
        suggested_docs_root: suggested,
    })
}

/// Persist global last-root and optionally write `{repo}/.docflow/project.json`.
pub fn open_project(repo_root: &str, docs_root: &str) -> Result<OpenedProject, ProjectError> {
    let repo = Path::new(repo_root);
    if !repo.is_dir() {
        return Err(ProjectError::NotADirectory(repo_root.to_string()));
    }
    let repo = repo.canonicalize().map_err(ProjectError::Canonicalize)?;

    let docs = Path::new(docs_root);
    if !docs.is_dir() {
        return Err(ProjectError::NotADirectory(docs_root.to_string()));
    }
    let docs = docs.canonicalize().map_err(ProjectError::Canonicalize)?;
    if !docs.starts_with(&repo) {
        return Err(ProjectError::DocsOutsideRepo(docs.display().to_string()));
    }

    let relative = paths::relative_to(&repo, &docs)?;
    let config = ProjectConfig::new(relative);
    let repo_str = repo.to_string_lossy().into_owned();
    project_store::save(&repo_str, &config)?;

    set_global_root(&repo_str).map_err(|e| ProjectError::Message(e.to_string()))?;

    Ok(OpenedProject {
        root: repo_str,
        docs_root: docs.to_string_lossy().into_owned(),
    })
}

/// Open using cached project.json only (no rewrite of docs root).
pub fn open_cached_project(repo_root: &str) -> Result<OpenedProject, ProjectError> {
    let repo = Path::new(repo_root);
    if !repo.is_dir() {
        return Err(ProjectError::NotADirectory(repo_root.to_string()));
    }
    let repo = repo.canonicalize().map_err(ProjectError::Canonicalize)?;
    let docs = load_cached_docs(&repo)?.ok_or_else(|| {
        ProjectError::Message("project.json missing or docs root invalid".into())
    })?;

    let repo_str = repo.to_string_lossy().into_owned();
    set_global_root(&repo_str).map_err(|e| ProjectError::Message(e.to_string()))?;

    Ok(OpenedProject {
        root: repo_str,
        docs_root: docs.to_string_lossy().into_owned(),
    })
}

pub fn get_project() -> Result<Option<OpenedProject>, ProjectError> {
    let settings = settings_store::load().map_err(|e| ProjectError::Message(e.to_string()))?;
    if !settings.general.restore_last_project {
        return Ok(None);
    }

    let Some(root) = settings.project.root.clone() else {
        return Ok(None);
    };

    let repo = PathBuf::from(&root);
    if !repo.is_dir() {
        clear_global_root().map_err(|e| ProjectError::Message(e.to_string()))?;
        return Ok(None);
    }

    let repo = repo.canonicalize().map_err(ProjectError::Canonicalize)?;
    let Some(docs) = load_cached_docs(&repo)? else {
        // Keep global root so UI can re-probe, but signal incomplete project.
        return Ok(None);
    };

    Ok(Some(OpenedProject {
        root: repo.to_string_lossy().into_owned(),
        docs_root: docs.to_string_lossy().into_owned(),
    }))
}

/// Returns the saved global root even when project.json is missing (for restore re-confirm).
pub fn get_saved_repo_root() -> Result<Option<String>, ProjectError> {
    let settings = settings_store::load().map_err(|e| ProjectError::Message(e.to_string()))?;
    if !settings.general.restore_last_project {
        return Ok(None);
    }
    let Some(root) = settings.project.root.clone() else {
        return Ok(None);
    };
    let path = PathBuf::from(&root);
    if !path.is_dir() {
        clear_global_root().map_err(|e| ProjectError::Message(e.to_string()))?;
        return Ok(None);
    }
    let canonical = path.canonicalize().map_err(ProjectError::Canonicalize)?;
    Ok(Some(canonical.to_string_lossy().into_owned()))
}

pub fn clear_project() -> Result<(), ProjectError> {
    clear_global_root().map_err(|e| ProjectError::Message(e.to_string()))
}

pub fn get_git_branch(repo_root: &str) -> Option<String> {
    git_repo::current_branch(Path::new(repo_root))
}

fn set_global_root(root: &str) -> Result<(), SettingsError> {
    let mut settings = settings_store::load().unwrap_or_default();
    settings.project.root = Some(root.to_string());
    settings_store::save(&settings)
}

fn clear_global_root() -> Result<(), SettingsError> {
    let mut settings = settings_store::load().unwrap_or_default();
    settings.project.root = None;
    settings_store::save(&settings)
}
