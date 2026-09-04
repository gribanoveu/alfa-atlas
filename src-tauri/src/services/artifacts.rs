//! Application-layer orchestration for artifacts — resolve the
//! repository-keyed storage directory, create/update/read/delete records.
//!
//! Provenance fields (`created_at_ms`, `chat_id`, `purpose`, `repo_root`)
//! are owned by this layer, not by whoever calls `save`: the builder UI
//! round-trips a whole record, and a stale or zeroed provenance field
//! coming back from it must not overwrite the truth on disk.

use uuid::Uuid;

use crate::domain::artifact::{
    repo_display_name as repo_folder_name, ArtifactContent, ArtifactError, ArtifactKind,
    ArtifactRecord, ArtifactStatus, ArtifactSummary,
};
use crate::domain::artifact_render::{self, RenderedArtifact};
use crate::infra::artifact_store;
use crate::services::repository_scope;

fn open_repo_id() -> Result<(String, String), ArtifactError> {
    repository_scope::open_repository().map_err(|e| ArtifactError::Project(e.to_string()))
}

/// The repository an existing artifact lives in — which is not necessarily
/// the one that is open.
///
/// Creating binds an artifact to the current project, but everything
/// afterwards is deliberately project-blind: a ticket written while one
/// service was open is routinely reopened, edited and published from
/// another, and an HTTP request assembled from a spec is finished in the
/// microservice's own repo. Scoping reads to the open project would have
/// made those artifacts unreachable without switching back.
fn owning_repo_id(artifact_id: &str) -> Result<String, ArtifactError> {
    artifact_store::find_repository(artifact_id)?
        .ok_or_else(|| ArtifactError::NotFound(artifact_id.to_string()))
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
    let repo_id = owning_repo_id(artifact_id)?;
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
        // Wholesale, except identity: a model rewriting a published ticket
        // sends the content back without its issue key, and losing the key
        // would make the next publish create a duplicate issue.
        content: content.with_identity_of(&stored.content),
        created_at_ms: stored.created_at_ms,
        updated_at_ms: 0,
        chat_id: stored.chat_id,
        repo_root: stored.repo_root,
    });
    artifact_store::save(&repo_id, &record)?;
    Ok(record)
}

/// Records the Jira key a publish produced.
///
/// The only writer of that field. Every other path — `save`,
/// `update_agent` — carries the stored key forward untouched (see
/// `ArtifactContent::with_identity_of`), so an artifact can only ever point
/// at an issue this app actually created.
pub fn record_issue_key(
    artifact_id: &str,
    issue_key: &str,
) -> Result<ArtifactRecord, ArtifactError> {
    let repo_id = owning_repo_id(artifact_id)?;
    let mut record = artifact_store::get(&repo_id, artifact_id)?;
    let ArtifactContent::JiraTicket(spec) = &mut record.content else {
        return Err(ArtifactError::Invalid(format!(
            "artifact {artifact_id} is not a Jira ticket and has no issue key"
        )));
    };
    spec.issue_key = issue_key.trim().to_string();
    let record = artifact_store::stamp_updated(record);
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
    let repo_id = owning_repo_id(&incoming.id)?;
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
        // Same rule as `update_agent`: the builder round-trips the whole
        // record, so a UI that ever stopped carrying the key would silently
        // unpublish the ticket.
        content: incoming.content.with_identity_of(&stored.content),
        created_at_ms: stored.created_at_ms,
        updated_at_ms: 0,
        chat_id: stored.chat_id,
        // Never re-derived from the open project: saving a ticket while a
        // different service is open must not re-home it.
        repo_root: stored.repo_root,
    });
    artifact_store::save(&repo_id, &record)?;
    Ok(record)
}

pub fn get(artifact_id: &str) -> Result<ArtifactRecord, ArtifactError> {
    let repo_id = owning_repo_id(artifact_id)?;
    artifact_store::get(&repo_id, artifact_id)
}

/// Every artifact of every repository. Filtering by project is the reader's
/// (and the model's) job — see `owning_repo_id`.
pub fn list() -> Result<Vec<ArtifactSummary>, ArtifactError> {
    artifact_store::list_all()
}

pub fn delete(artifact_id: &str) -> Result<(), ArtifactError> {
    let repo_id = owning_repo_id(artifact_id)?;
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
    use crate::infra::settings_store::test_support::with_temp_home;

    fn stored_in(repo_id: &str, artifact_id: &str) -> ArtifactRecord {
        let record = artifact_store::stamp_new(ArtifactRecord {
            id: artifact_id.to_string(),
            kind: ArtifactKind::HttpRequest,
            title: "Создание документа".into(),
            purpose: None,
            status: ArtifactStatus::Draft,
            content: ArtifactContent::HttpRequest(HttpRequestSpec::default()),
            created_at_ms: 0,
            updated_at_ms: 0,
            chat_id: None,
            repo_root: Some("/repos/corp-wlbuh-enp-api".into()),
        });
        artifact_store::save(repo_id, &record).expect("save");
        record
    }

    #[test]
    fn an_artifact_is_reachable_without_its_project_being_open() {
        // No project is open here at all, which is the strongest form of
        // "not the current repository": before this, every read went
        // through `open_repository()` and would have failed outright.
        with_temp_home(|| {
            stored_in("some-other-repo", "written-elsewhere");
            let loaded = get("written-elsewhere").expect("get");
            assert_eq!(loaded.id, "written-elsewhere");
        });
    }

    #[test]
    fn a_missing_artifact_is_not_found_rather_than_a_project_error() {
        with_temp_home(|| {
            assert!(matches!(get("nothing-here"), Err(ArtifactError::NotFound(id)) if id == "nothing-here"));
        });
    }

    #[test]
    fn saving_keeps_an_artifact_in_the_repository_that_owns_it() {
        with_temp_home(|| {
            let stored = stored_in("owning-repo", "stays-put");
            let saved = save(ArtifactRecord {
                title: "Переименовано".into(),
                ..stored
            })
            .expect("save");
            assert_eq!(saved.title, "Переименовано");
            // Still one artifact, still in the same directory — a save made
            // from a different project must not copy it into that one.
            assert_eq!(
                artifact_store::find_repository("stays-put").expect("find"),
                Some("owning-repo".to_string())
            );
            assert_eq!(saved.repo_root.as_deref(), Some("/repos/corp-wlbuh-enp-api"));
        });
    }

    #[test]
    fn deleting_reaches_an_artifact_of_another_project() {
        with_temp_home(|| {
            stored_in("some-other-repo", "doomed");
            delete("doomed").expect("delete");
            assert!(matches!(get("doomed"), Err(ArtifactError::NotFound(_))));
        });
    }

    fn ticket_in(repo_id: &str, artifact_id: &str, why: &str) -> ArtifactRecord {
        let record = artifact_store::stamp_new(ArtifactRecord {
            id: artifact_id.to_string(),
            kind: ArtifactKind::JiraTicket,
            title: "Тикет".into(),
            purpose: None,
            status: ArtifactStatus::Ready,
            content: ArtifactContent::JiraTicket(crate::domain::artifact::JiraTicketSpec {
                why: why.to_string(),
                ..Default::default()
            }),
            created_at_ms: 0,
            updated_at_ms: 0,
            chat_id: None,
            repo_root: Some("/repos/corp-wlbuh-enp-api".into()),
        });
        artifact_store::save(repo_id, &record).expect("save");
        record
    }

    fn issue_key_of(record: &ArtifactRecord) -> &str {
        match &record.content {
            ArtifactContent::JiraTicket(spec) => &spec.issue_key,
            _ => panic!("not a ticket"),
        }
    }

    /// Editing a published ticket must not un-publish it: the key is what
    /// stops the next click from creating a second issue in Jira.
    #[test]
    fn an_assistant_rewrite_keeps_a_published_ticket_published() {
        with_temp_home(|| {
            ticket_in("repo", "published", "Старая формулировка");
            record_issue_key("published", "WOWTAX-8094").expect("record key");

            // The model sends the whole ticket back, without the key — it is
            // not in the tool's schema for it to know about.
            let updated = update_agent(
                "published",
                None,
                ArtifactContent::JiraTicket(crate::domain::artifact::JiraTicketSpec {
                    why: "Новая формулировка".into(),
                    ..Default::default()
                }),
            )
            .expect("update");

            assert_eq!(issue_key_of(&updated), "WOWTAX-8094");
            assert_eq!(issue_key_of(&get("published").expect("get")), "WOWTAX-8094");
        });
    }

    #[test]
    fn saving_from_the_builder_keeps_the_issue_key_too() {
        with_temp_home(|| {
            let stored = ticket_in("repo", "published", "Проблема");
            record_issue_key("published", "WOWTAX-8094").expect("record key");

            // A record that lost the key on its way through the UI.
            let saved = save(ArtifactRecord {
                content: ArtifactContent::JiraTicket(
                    crate::domain::artifact::JiraTicketSpec {
                        why: "Проблема".into(),
                        ..Default::default()
                    },
                ),
                ..stored
            })
            .expect("save");
            assert_eq!(issue_key_of(&saved), "WOWTAX-8094");
        });
    }

    #[test]
    fn an_issue_key_cannot_be_claimed_by_writing_content() {
        with_temp_home(|| {
            ticket_in("repo", "draft", "Проблема");
            let updated = update_agent(
                "draft",
                None,
                ArtifactContent::JiraTicket(crate::domain::artifact::JiraTicketSpec {
                    issue_key: "WOWTAX-1".into(),
                    why: "Проблема".into(),
                    ..Default::default()
                }),
            )
            .expect("update");
            assert!(issue_key_of(&updated).is_empty());
        });
    }

    #[test]
    fn record_issue_key_refuses_anything_but_a_ticket() {
        with_temp_home(|| {
            stored_in("repo", "an-http-request");
            assert!(matches!(
                record_issue_key("an-http-request", "WOWTAX-1"),
                Err(ArtifactError::Invalid(_))
            ));
        });
    }

    #[test]
    fn list_returns_artifacts_from_every_project() {
        with_temp_home(|| {
            stored_in("repo-a", "one");
            stored_in("repo-b", "two");
            let ids: Vec<_> = list().expect("list").into_iter().map(|s| s.id).collect();
            assert_eq!(ids.len(), 2);
            assert!(ids.contains(&"one".to_string()) && ids.contains(&"two".to_string()));
        });
    }

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
