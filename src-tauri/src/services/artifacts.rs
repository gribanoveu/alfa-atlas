//! Application-layer orchestration for artifacts — resolve the
//! repository-keyed storage directory, create/update/read/delete records.
//!
//! Provenance fields (`created_at_ms`, `chat_id`, `purpose`, `repo_root`)
//! are owned by this layer, not by whoever calls `save`: the builder UI
//! round-trips a whole record, and a stale or zeroed provenance field
//! coming back from it must not overwrite the truth on disk.

use uuid::Uuid;

use crate::domain::artifact::{
    ArtifactContent, ArtifactError, ArtifactKind, ArtifactRecord, ArtifactStatus, ArtifactSummary,
};
use crate::domain::artifact_render::{self, RenderedArtifact};
use crate::infra::artifact_store;
use crate::services::repository_scope;

fn open_repo_id() -> Result<(String, String), ArtifactError> {
    repository_scope::open_repository().map_err(|e| ArtifactError::Project(e.to_string()))
}

fn seed_repo_path_default(content: &mut ArtifactContent, repo_root: &str) {
    match content {
        // Leading slash to match every other path in this builder (the
        // placeholder text, the templates, `looks_like_endpoint`'s check) —
        // a path is always slash-rooted here, never a bare relative segment.
        ArtifactContent::HttpRequest(spec) if spec.path.trim().is_empty() => {
            spec.path = format!("/{}/api/", repo_folder_name(repo_root));
        }
        ArtifactContent::HttpRequest(_) => {}
        // Nothing about a ticket is derivable from the repository path.
        ArtifactContent::JiraTicket(_) => {}
    }
}

/// The repo's own name, not its identity hash — just the last path segment
/// of its root, matching what the project switcher in the top bar already
/// shows the user (e.g. `/Users/x/WORK_REPOS/.../corp-wlbuh-ausn-api` →
/// `corp-wlbuh-ausn-api`).
fn repo_folder_name(repo_root: &str) -> &str {
    std::path::Path::new(repo_root)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(repo_root)
}

/// A fresh `Draft`. `prefill` is whatever the requesting model already knew
/// (method, path, the standard header block) — it only seeds the form, and
/// is dropped if its kind disagrees with `kind` rather than being coerced.
pub fn create_draft(
    kind: ArtifactKind,
    title: String,
    purpose: Option<String>,
    prefill: Option<ArtifactContent>,
    chat_id: Option<String>,
) -> Result<ArtifactRecord, ArtifactError> {
    let (repo_id, repo_root) = open_repo_id()?;
    let mut content = match prefill {
        Some(content) if content.kind() == kind => content,
        _ => ArtifactContent::empty_for(kind),
    };
    // Repo context isn't known to `domain::artifact` (no I/O there), so this
    // seed lives here rather than in `ArtifactContent::empty_for`/
    // `HttpRequestSpec::default()` — unlike the static `{host}` placeholder,
    // the `<сервис>` segment of the house endpoint convention
    // (`https://{host}/<сервис>/<путь>/...`) is a real, known value for the
    // open repo, so it's filled in literally rather than left as a token.
    // Only when the model's own prefill didn't already say something.
    seed_repo_path_default(&mut content, &repo_root);
    let title = title.trim();
    let record = artifact_store::stamp_new(ArtifactRecord {
        id: Uuid::new_v4().to_string(),
        kind,
        title: if title.is_empty() {
            default_title(kind)
        } else {
            title.to_string()
        },
        purpose: purpose.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()),
        status: ArtifactStatus::Draft,
        content,
        created_at_ms: 0,
        updated_at_ms: 0,
        chat_id,
        repo_root: Some(repo_root),
    });
    artifact_store::save(&repo_id, &record)?;
    Ok(record)
}

fn default_title(kind: ArtifactKind) -> String {
    match kind {
        ArtifactKind::HttpRequest => "Новый HTTP-запрос".to_string(),
        ArtifactKind::JiraTicket => "Новый тикет".to_string(),
    }
}

/// The assistant authoring an artifact outright, rather than asking the user
/// to fill one in: `create_agent` writes finished content, `update_agent`
/// rewrites it.
///
/// Guarded by `ArtifactKind::is_agent_authored`, so the model cannot reach
/// for this to invent an HTTP request table — see that method's doc comment.
/// Created `Ready` rather than `Draft`: the content is complete when it
/// arrives, and `Draft` means "the user is still filling this in".
pub fn create_agent(
    kind: ArtifactKind,
    title: String,
    content: ArtifactContent,
    chat_id: Option<String>,
) -> Result<ArtifactRecord, ArtifactError> {
    ensure_agent_authored(kind)?;
    if content.kind() != kind {
        return Err(ArtifactError::Invalid(
            "artifact kind does not match its content".into(),
        ));
    }
    let (repo_id, repo_root) = open_repo_id()?;
    let title = title.trim();
    let record = artifact_store::stamp_new(ArtifactRecord {
        id: Uuid::new_v4().to_string(),
        kind,
        title: if title.is_empty() {
            default_title(kind)
        } else {
            title.to_string()
        },
        purpose: None,
        status: ArtifactStatus::Ready,
        content,
        created_at_ms: 0,
        updated_at_ms: 0,
        chat_id,
        repo_root: Some(repo_root),
    });
    artifact_store::save(&repo_id, &record)?;
    Ok(record)
}

/// Whole-content replacement, not a merge: a model rewriting one section
/// sends the whole ticket back, and a field-wise merge would make "remove
/// this risk" impossible to express. `title` is left alone when `None`.
pub fn update_agent(
    artifact_id: &str,
    title: Option<String>,
    content: ArtifactContent,
) -> Result<ArtifactRecord, ArtifactError> {
    let (repo_id, _) = open_repo_id()?;
    let stored = artifact_store::get(&repo_id, artifact_id)?;
    ensure_agent_authored(stored.kind)?;
    if content.kind() != stored.kind {
        return Err(ArtifactError::Invalid(format!(
            "artifact {artifact_id} is a {:?}, cannot be updated with different content",
            stored.kind
        )));
    }

    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or(stored.title);
    let record = artifact_store::stamp_updated(ArtifactRecord {
        id: stored.id,
        kind: stored.kind,
        title,
        purpose: stored.purpose,
        status: stored.status,
        content,
        created_at_ms: stored.created_at_ms,
        updated_at_ms: 0,
        chat_id: stored.chat_id,
        repo_root: stored.repo_root,
    });
    artifact_store::save(&repo_id, &record)?;
    Ok(record)
}

fn ensure_agent_authored(kind: ArtifactKind) -> Result<(), ArtifactError> {
    if kind.is_agent_authored() {
        return Ok(());
    }
    Err(ArtifactError::Invalid(format!(
        "artifacts of kind {kind:?} are filled in by the user — use requestArtifact instead of writing one"
    )))
}

/// Persist edits. Only `title`, `status` and `content` are taken from
/// `incoming`; everything else is preserved from the stored record.
pub fn save(incoming: ArtifactRecord) -> Result<ArtifactRecord, ArtifactError> {
    // A record whose tag disagrees with its payload would be persisted and
    // later render as the wrong kind.
    if incoming.kind != incoming.content.kind() {
        return Err(ArtifactError::Invalid(
            "artifact kind does not match its content".into(),
        ));
    }
    let (repo_id, repo_root) = open_repo_id()?;
    let stored = artifact_store::get(&repo_id, &incoming.id)?;

    let title = incoming.title.trim();
    let record = artifact_store::stamp_updated(ArtifactRecord {
        id: stored.id,
        kind: incoming.kind,
        title: if title.is_empty() {
            stored.title
        } else {
            title.to_string()
        },
        purpose: stored.purpose,
        status: incoming.status,
        content: incoming.content,
        created_at_ms: stored.created_at_ms,
        updated_at_ms: 0,
        chat_id: stored.chat_id,
        repo_root: stored.repo_root.or(Some(repo_root)),
    });
    artifact_store::save(&repo_id, &record)?;
    Ok(record)
}

pub fn get(artifact_id: &str) -> Result<ArtifactRecord, ArtifactError> {
    let (repo_id, _) = open_repo_id()?;
    artifact_store::get(&repo_id, artifact_id)
}

pub fn list() -> Result<Vec<ArtifactSummary>, ArtifactError> {
    let (repo_id, _) = open_repo_id()?;
    artifact_store::list(&repo_id)
}

pub fn delete(artifact_id: &str) -> Result<(), ArtifactError> {
    let (repo_id, _) = open_repo_id()?;
    artifact_store::delete(&repo_id, artifact_id)
}

pub fn render(content: &ArtifactContent) -> RenderedArtifact {
    artifact_render::render(content)
}

/// Used by Settings → Paths; does not require an open project.
pub fn artifacts_root_path() -> Result<std::path::PathBuf, ArtifactError> {
    Ok(crate::infra::settings_store::settings_dir()?.join("artifacts"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::artifact::HttpRequestSpec;

    #[test]
    fn repo_folder_name_takes_the_last_path_segment() {
        assert_eq!(
            repo_folder_name("/Users/x/WORK_REPOS/WLBUH/corp-wlbuh-ausn-api"),
            "corp-wlbuh-ausn-api"
        );
    }

    #[test]
    fn repo_folder_name_tolerates_a_trailing_slash() {
        assert_eq!(repo_folder_name("/repos/corp-wlbuh-ausn-api/"), "corp-wlbuh-ausn-api");
    }

    #[test]
    fn repo_folder_name_falls_back_to_the_whole_string_when_it_has_no_segment() {
        // `Path::file_name()` returns `None` for `/`, `.`, `..` — the
        // fallback keeps this a total function rather than an empty string.
        assert_eq!(repo_folder_name("/"), "/");
    }

    #[test]
    fn seed_repo_path_default_fills_an_empty_path() {
        let mut content =
            ArtifactContent::HttpRequest(HttpRequestSpec { path: String::new(), ..Default::default() });
        seed_repo_path_default(&mut content, "/repos/corp-wlbuh-ausn-api");
        let ArtifactContent::HttpRequest(spec) = content else { panic!("seeded content must stay an httpRequest") };
        assert_eq!(spec.path, "/corp-wlbuh-ausn-api/api/");
    }

    #[test]
    fn seed_repo_path_default_treats_whitespace_as_empty() {
        let mut content =
            ArtifactContent::HttpRequest(HttpRequestSpec { path: "   ".into(), ..Default::default() });
        seed_repo_path_default(&mut content, "/repos/corp-wlbuh-ausn-api");
        let ArtifactContent::HttpRequest(spec) = content else { panic!("seeded content must stay an httpRequest") };
        assert_eq!(spec.path, "/corp-wlbuh-ausn-api/api/");
    }

    #[test]
    fn seed_repo_path_default_never_overwrites_a_real_path() {
        // Whether the user typed it or the model's own `requestArtifact`
        // prefill already named one — either way it must survive.
        let mut content = ArtifactContent::HttpRequest(HttpRequestSpec {
            path: "/v1/documents".into(),
            ..Default::default()
        });
        seed_repo_path_default(&mut content, "/repos/corp-wlbuh-ausn-api");
        let ArtifactContent::HttpRequest(spec) = content else { panic!("seeded content must stay an httpRequest") };
        assert_eq!(spec.path, "/v1/documents");
    }
}
