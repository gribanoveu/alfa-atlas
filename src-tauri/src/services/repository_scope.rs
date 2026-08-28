//! "Which repository is this?" — the stable identity string used as the
//! folder name under `~/.atlas/plans/`, `~/.atlas/artifacts/` and
//! `~/.atlas/embeddings/`.
//!
//! Lives here rather than in each of those services because resolving it
//! can *mint* an id: a repo with no git remote falls back to a random UUID
//! persisted as `ProjectConfig::local_repository_id`. Two independent
//! copies of that fallback could each mint a different id for the same
//! repo and silently split its stored state across two folders, so there is
//! exactly one implementation.

use std::path::Path;

use uuid::Uuid;

use crate::domain::project_config::ProjectError;
use crate::infra::{project_store, repository_identity};
use crate::services::project_open;

/// Identity for an arbitrary repo root. Prefers the canonicalized git
/// remote (so the same repo cloned twice shares its state), falling back to
/// a per-checkout UUID.
pub fn resolve_repository_id(repo_root: &Path) -> Result<String, ProjectError> {
    let identity = repository_identity::resolve(repo_root);
    let source = match identity.canonical_url {
        Some(url) => url,
        None => local_identity(repo_root)?,
    };
    Ok(repository_identity::repository_id(&source))
}

fn local_identity(repo_root: &Path) -> Result<String, ProjectError> {
    let root_str = repo_root
        .to_str()
        .ok_or_else(|| ProjectError::Message("repo root is not valid UTF-8".into()))?;
    let mut config = project_store::load(root_str)?
        .ok_or_else(|| ProjectError::Message(format!("no project.json found for {root_str}")))?;

    if let Some(id) = config.local_repository_id.clone() {
        return Ok(id);
    }

    let id = Uuid::new_v4().to_string();
    config.local_repository_id = Some(id.clone());
    project_store::save(root_str, &config)?;
    Ok(id)
}

/// `(repository_id, repo_root)` for the currently open project. Errors with
/// a message containing `"no project is open"` when there is none — the
/// same marker `services::ai_tools::current_scope` already establishes, and
/// which callers grep for to stay quiet about an expected condition.
pub fn open_repository() -> Result<(String, String), ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".into()))?;
    let repo_id = resolve_repository_id(Path::new(&opened.root))?;
    Ok((repo_id, opened.root))
}
