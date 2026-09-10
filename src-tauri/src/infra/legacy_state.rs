//! One-time cleanup of state written by the Jira integration this app no
//! longer has.
//!
//! Two things outlive the code that wrote them, and both mislead rather than
//! merely sit there:
//!
//! * a ticket artifact's `issueKey` — the issue a *publish* created. Nothing
//!   opens that issue any more, and the rules that deliberately kept the key
//!   across edits went with publishing, so a ticket still carrying one reads
//!   as published to the user and to the model while being only a draft;
//! * the encrypted API token in `jira_credentials.enc`, a secret with no
//!   reader left in the binary.
//!
//! Both are dealt with in place, once, at startup. The key is dropped by
//! rewriting the ticket through `artifact_store::save` — which is also what
//! leaves the file in the canonical on-disk shape — and the token file is
//! deleted. Idempotent: a second run finds nothing to do, and a file that
//! never carried publish state is not touched at all.
//!
//! Failures are reported, never fatal: the app is perfectly usable with a
//! stale artifact file on disk, and refusing to start over one would be a
//! worse trade than leaving it behind.

use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::artifact::{ArtifactError, ArtifactRecord};
use crate::domain::settings::SettingsError;
use crate::infra::{artifact_store, settings_store};

/// What `infra::jira_credentials_store` used to write into settings dir.
const LEGACY_TOKEN_FILE: &str = "jira_credentials.enc";

/// The publish result `JiraTicketSpec` used to carry, as it appears in a
/// stored record. Only ever used to decide whether a file needs rewriting.
const LEGACY_KEY_FIELD: &str = "\"issueKey\"";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct CleanupReport {
    /// Ticket files that carried publish state and were rewritten.
    pub tickets_rewritten: usize,
    /// Whether the orphaned token file was still there to delete.
    pub token_removed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum LegacyStateError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Artifact(#[from] ArtifactError),
    #[error(transparent)]
    Settings(#[from] SettingsError),
}

pub fn clean_up() -> Result<CleanupReport, LegacyStateError> {
    Ok(CleanupReport {
        tickets_rewritten: strip_publish_state()?,
        token_removed: remove_token_file()?,
    })
}

/// Walks `~/.atlas/artifacts/{repo}/{id}.json` and rewrites every ticket that
/// still carries a key.
fn strip_publish_state() -> Result<usize, LegacyStateError> {
    let root = artifact_store::artifacts_root()?;
    if !root.is_dir() {
        return Ok(0);
    }

    let mut rewritten = 0;
    for repo_entry in fs::read_dir(&root)? {
        let repo_entry = repo_entry?;
        if !repo_entry.path().is_dir() {
            continue;
        }
        // A directory whose name is not valid UTF-8 cannot be addressed by
        // the store either, so there is nothing of its to rewrite.
        let Some(repo_id) = repo_entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        for entry in fs::read_dir(repo_entry.path())? {
            let path = entry?.path();
            if !path.is_file() {
                continue;
            }
            // Anything that is not an artifact record — a temp file from a
            // crashed write, a stray note — is not this pass's business.
            if artifact_id_of(&path).is_none() {
                continue;
            }
            if rewrite_without_publish_state(&repo_id, &path)? {
                rewritten += 1;
            }
        }
    }
    Ok(rewritten)
}

/// `{id}.json`, or `None` for anything else in the directory — including the
/// `<id>.tmp` a crashed write can leave behind.
fn artifact_id_of(path: &Path) -> Option<String> {
    if path.extension().and_then(|e| e.to_str()) != Some("json") {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    if name.starts_with('.') {
        return None;
    }
    Some(name.trim_end_matches(".json").to_string())
}

/// `Ok(true)` when the file carried publish state and was rewritten.
///
/// A file that will not parse is left alone: this pass is not the place to
/// fail over something the store itself already skips.
fn rewrite_without_publish_state(repo_id: &str, path: &Path) -> Result<bool, LegacyStateError> {
    let raw = fs::read_to_string(path)?;
    if !raw.contains(LEGACY_KEY_FIELD) {
        return Ok(false);
    }
    let Ok(record) = serde_json::from_str::<ArtifactRecord>(&raw) else {
        return Ok(false);
    };
    artifact_store::save(repo_id, &record)?;
    Ok(true)
}

fn remove_token_file() -> Result<bool, LegacyStateError> {
    let path: PathBuf = settings_store::settings_dir()?.join(LEGACY_TOKEN_FILE);
    match fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::artifact::{
        ArtifactContent, ArtifactKind, ArtifactStatus, HttpRequestSpec, JiraTicketSpec,
    };
    use crate::infra::settings_store::test_support::with_temp_home;

    fn ticket_record(artifact_id: &str) -> ArtifactRecord {
        ArtifactRecord {
            id: artifact_id.to_string(),
            kind: ArtifactKind::JiraTicket,
            title: "Тикет".into(),
            purpose: None,
            status: ArtifactStatus::Ready,
            content: ArtifactContent::JiraTicket(JiraTicketSpec {
                why: "Проблема".into(),
                ..Default::default()
            }),
            created_at_ms: 0,
            updated_at_ms: 0,
            chat_id: None,
            repo_root: None,
        }
    }

    /// Saves a ticket the way the removed publish path left it: with the
    /// issue key sitting next to the rest of the content.
    fn stored_ticket_with_key(repo_id: &str, artifact_id: &str) -> PathBuf {
        artifact_store::save(repo_id, &ticket_record(artifact_id)).expect("save");
        let path = artifact_store::artifact_dir(repo_id)
            .expect("dir")
            .join(format!("{artifact_id}.json"));
        let raw = fs::read_to_string(&path).expect("read");
        let marker = "\"content\": {";
        let at = raw.find(marker).expect("content block") + marker.len();
        let (head, tail) = raw.split_at(at);
        fs::write(
            &path,
            format!("{head}\n    \"issueKey\": \"WOWTAX-8094\",{tail}"),
        )
        .expect("patch");
        path
    }

    #[test]
    fn drops_the_publish_state_and_the_orphaned_token() {
        with_temp_home(|| {
            let path = stored_ticket_with_key("repo", "published");
            let token = settings_store::ensure_settings_dir()
                .expect("settings dir")
                .join(LEGACY_TOKEN_FILE);
            fs::write(&token, b"sealed").expect("token");

            let report = clean_up().expect("clean up");
            assert_eq!(report.tickets_rewritten, 1);
            assert!(report.token_removed);
            assert!(!token.exists());

            let raw = fs::read_to_string(&path).expect("read");
            assert!(!raw.contains(LEGACY_KEY_FIELD), "key survived: {raw}");
            // The ticket itself is untouched — only the publish result goes.
            let stored = artifact_store::get("repo", "published").expect("get");
            let ArtifactContent::JiraTicket(spec) = stored.content else {
                panic!("not a ticket any more");
            };
            assert_eq!(spec.why, "Проблема");
        });
    }

    #[test]
    fn a_second_run_has_nothing_to_do() {
        with_temp_home(|| {
            stored_ticket_with_key("repo", "published");
            let token = settings_store::ensure_settings_dir()
                .expect("settings dir")
                .join(LEGACY_TOKEN_FILE);
            fs::write(&token, b"sealed").expect("token");

            clean_up().expect("first run");
            let report = clean_up().expect("second run");
            assert_eq!(report, CleanupReport::default());
        });
    }

    /// The pass must not rewrite files that have nothing to strip: an
    /// untouched artifact keeps its bytes, timestamps and all.
    #[test]
    fn leaves_unaffected_artifacts_alone() {
        with_temp_home(|| {
            artifact_store::save(
                "repo",
                &ArtifactRecord {
                    kind: ArtifactKind::HttpRequest,
                    content: ArtifactContent::HttpRequest(HttpRequestSpec::default()),
                    ..ticket_record("request")
                },
            )
            .expect("save");
            let path = artifact_store::artifact_dir("repo")
                .expect("dir")
                .join("request.json");
            let before = fs::read_to_string(&path).expect("read");

            let report = clean_up().expect("clean up");

            assert_eq!(report.tickets_rewritten, 0);
            assert!(!report.token_removed);
            assert_eq!(fs::read_to_string(&path).expect("read"), before);
        });
    }

    /// A file that mentions the field but does not parse is not this pass's
    /// problem — it is skipped, not deleted and not fatal.
    #[test]
    fn a_broken_file_is_skipped() {
        with_temp_home(|| {
            let dir = artifact_store::artifact_dir("repo").expect("dir");
            fs::create_dir_all(&dir).expect("mkdir");
            let path = dir.join("broken.json");
            fs::write(&path, "{\"issueKey\": \"WOWTAX-1\",").expect("write broken");

            let report = clean_up().expect("clean up");

            assert_eq!(report.tickets_rewritten, 0);
            assert!(path.is_file(), "a broken file must be left where it was");
        });
    }
}
