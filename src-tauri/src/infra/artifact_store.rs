//! File-backed store for artifacts under
//! `~/.atlas/artifacts/{repository_id}/{artifact_id}.json`.
//!
//! Same shape as `plan_store`: one JSON file per record, atomic temp+rename
//! writes, ids validated before they ever reach the filesystem.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::artifact::{ArtifactError, ArtifactRecord, ArtifactSummary};
use crate::infra::settings_store;

const ARTIFACTS_DIR_NAME: &str = "artifacts";

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `~/.atlas/artifacts/{repository_id}/`
pub fn artifact_dir(repository_id: &str) -> Result<PathBuf, ArtifactError> {
    Ok(settings_store::settings_dir()?
        .join(ARTIFACTS_DIR_NAME)
        .join(repository_id))
}

fn artifact_path(repository_id: &str, artifact_id: &str) -> Result<PathBuf, ArtifactError> {
    // Ids are generated server-side, but the id also arrives from the
    // frontend on save/read — guard against traversal regardless of who
    // supplied it, same as `plan_store`.
    if artifact_id.is_empty()
        || artifact_id.contains('/')
        || artifact_id.contains('\\')
        || artifact_id.contains("..")
    {
        return Err(ArtifactError::Invalid(format!(
            "invalid artifact id: {artifact_id}"
        )));
    }
    Ok(artifact_dir(repository_id)?.join(format!("{artifact_id}.json")))
}

/// Atomic write: temp file in the same directory, then rename.
pub fn save(repository_id: &str, record: &ArtifactRecord) -> Result<(), ArtifactError> {
    let dir = artifact_dir(repository_id)?;
    fs::create_dir_all(&dir)?;
    let path = artifact_path(repository_id, &record.id)?;
    let tmp = dir.join(format!(".{}.tmp", record.id));
    let json = serde_json::to_string_pretty(record)?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn get(repository_id: &str, artifact_id: &str) -> Result<ArtifactRecord, ArtifactError> {
    let path = artifact_path(repository_id, artifact_id)?;
    if !path.is_file() {
        return Err(ArtifactError::NotFound(artifact_id.to_string()));
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn delete(repository_id: &str, artifact_id: &str) -> Result<(), ArtifactError> {
    let path = artifact_path(repository_id, artifact_id)?;
    if !path.is_file() {
        return Err(ArtifactError::NotFound(artifact_id.to_string()));
    }
    fs::remove_file(&path)?;
    Ok(())
}

/// All artifacts for a repository, newest `updated_at_ms` first.
pub fn list(repository_id: &str) -> Result<Vec<ArtifactSummary>, ArtifactError> {
    let dir = artifact_dir(repository_id)?;
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // Skip temp files left by a crashed write.
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'))
        {
            continue;
        }
        match load_summary_from_path(&path) {
            Ok(summary) => out.push(summary),
            Err(_) => continue,
        }
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.updated_at_ms));
    Ok(out)
}

fn load_summary_from_path(path: &Path) -> Result<ArtifactSummary, ArtifactError> {
    let raw = fs::read_to_string(path)?;
    let record: ArtifactRecord = serde_json::from_str(&raw)?;
    Ok(record.to_summary())
}

pub fn stamp_new(mut record: ArtifactRecord) -> ArtifactRecord {
    let now = now_millis();
    record.created_at_ms = now;
    record.updated_at_ms = now;
    record
}

pub fn stamp_updated(mut record: ArtifactRecord) -> ArtifactRecord {
    record.updated_at_ms = now_millis();
    record
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::artifact::{
        ArtifactContent, ArtifactKind, ArtifactStatus, HttpRequestSpec,
    };
    use crate::infra::settings_store::test_support::with_temp_home;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_repo_id() -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        format!(
            "test-artifact-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn sample_record(id: &str) -> ArtifactRecord {
        ArtifactRecord {
            id: id.to_string(),
            kind: ArtifactKind::HttpRequest,
            title: "Создание документа".into(),
            purpose: Some("Нужны входные параметры".into()),
            status: ArtifactStatus::Draft,
            content: ArtifactContent::HttpRequest(HttpRequestSpec {
                method: "POST".into(),
                path: "/api/documents".into(),
                ..Default::default()
            }),
            created_at_ms: 0,
            updated_at_ms: 0,
            chat_id: None,
            repo_root: Some("/repo".into()),
        }
    }

    #[test]
    fn save_then_get_round_trips() {
        with_temp_home(|| {
            let repo = unique_repo_id();
            let record = stamp_new(sample_record("a1"));
            save(&repo, &record).expect("save");
            let loaded = get(&repo, "a1").expect("get");
            assert_eq!(loaded, record);
        });
    }

    #[test]
    fn get_missing_is_not_found() {
        with_temp_home(|| {
            let repo = unique_repo_id();
            assert!(matches!(
                get(&repo, "nope"),
                Err(ArtifactError::NotFound(id)) if id == "nope"
            ));
        });
    }

    #[test]
    fn list_is_newest_first_and_empty_for_an_unknown_repo() {
        with_temp_home(|| {
            let repo = unique_repo_id();
            assert!(list(&repo).expect("list").is_empty());

            let mut older = sample_record("older");
            older.updated_at_ms = 100;
            let mut newer = sample_record("newer");
            newer.updated_at_ms = 200;
            save(&repo, &older).expect("save older");
            save(&repo, &newer).expect("save newer");

            let listed = list(&repo).expect("list");
            assert_eq!(
                listed.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
                vec!["newer", "older"]
            );
            assert_eq!(listed[0].subtitle, "POST /api/documents");
        });
    }

    #[test]
    fn delete_removes_the_record() {
        with_temp_home(|| {
            let repo = unique_repo_id();
            save(&repo, &sample_record("gone")).expect("save");
            delete(&repo, "gone").expect("delete");
            assert!(matches!(get(&repo, "gone"), Err(ArtifactError::NotFound(_))));
            assert!(matches!(
                delete(&repo, "gone"),
                Err(ArtifactError::NotFound(_))
            ));
        });
    }

    #[test]
    fn traversal_ids_are_rejected_before_touching_the_filesystem() {
        with_temp_home(|| {
            let repo = unique_repo_id();
            for bad in ["", "../escape", "a/b", "a\\b", ".."] {
                assert!(
                    matches!(get(&repo, bad), Err(ArtifactError::Invalid(_))),
                    "id {bad:?} should be rejected"
                );
            }
        });
    }

    #[test]
    fn list_skips_unparseable_and_temp_files() {
        with_temp_home(|| {
            let repo = unique_repo_id();
            save(&repo, &sample_record("good")).expect("save");
            let dir = artifact_dir(&repo).expect("dir");
            fs::write(dir.join("broken.json"), "{ not json").expect("write broken");
            fs::write(dir.join(".half.tmp"), "{}").expect("write temp");
            fs::write(dir.join("notes.txt"), "hello").expect("write txt");

            let listed = list(&repo).expect("list");
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].id, "good");
        });
    }
}
