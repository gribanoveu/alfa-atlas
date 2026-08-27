//! File-backed store for work plans under
//! `~/.atlas/plans/{repository_id}/{plan_id}.json`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::plan::{PlanError, PlanRecord, PlanSummary};
use crate::infra::settings_store;

const PLANS_DIR_NAME: &str = "plans";

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `~/.atlas/plans/{repository_id}/`
pub fn plan_dir(repository_id: &str) -> Result<PathBuf, PlanError> {
    Ok(settings_store::settings_dir()?
        .join(PLANS_DIR_NAME)
        .join(repository_id))
}

fn plan_path(repository_id: &str, plan_id: &str) -> Result<PathBuf, PlanError> {
    // Guard against path traversal in a model-supplied id.
    if plan_id.is_empty()
        || plan_id.contains('/')
        || plan_id.contains('\\')
        || plan_id.contains("..")
    {
        return Err(PlanError::Invalid(format!("invalid plan id: {plan_id}")));
    }
    Ok(plan_dir(repository_id)?.join(format!("{plan_id}.json")))
}

/// Atomic write: temp file in the same directory, then rename.
pub fn save(repository_id: &str, record: &PlanRecord) -> Result<(), PlanError> {
    let dir = plan_dir(repository_id)?;
    fs::create_dir_all(&dir)?;
    let path = plan_path(repository_id, &record.id)?;
    let tmp = dir.join(format!(".{}.tmp", record.id));
    let json = serde_json::to_string_pretty(record)?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn get(repository_id: &str, plan_id: &str) -> Result<PlanRecord, PlanError> {
    let path = plan_path(repository_id, plan_id)?;
    if !path.is_file() {
        return Err(PlanError::NotFound(plan_id.to_string()));
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn delete(repository_id: &str, plan_id: &str) -> Result<(), PlanError> {
    let path = plan_path(repository_id, plan_id)?;
    if !path.is_file() {
        return Err(PlanError::NotFound(plan_id.to_string()));
    }
    fs::remove_file(&path)?;
    Ok(())
}

/// All plans for a repository, newest `updated_at_ms` first.
pub fn list(repository_id: &str) -> Result<Vec<PlanSummary>, PlanError> {
    let dir = plan_dir(repository_id)?;
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
    out.sort_by_key(|p| std::cmp::Reverse(p.updated_at_ms));
    Ok(out)
}

fn load_summary_from_path(path: &Path) -> Result<PlanSummary, PlanError> {
    let raw = fs::read_to_string(path)?;
    let record: PlanRecord = serde_json::from_str(&raw)?;
    Ok(record.to_summary())
}

/// Stamp `updated_at_ms` (and optionally `created_at_ms` for new records).
pub fn stamp_new(mut record: PlanRecord) -> PlanRecord {
    let now = now_millis();
    record.created_at_ms = now;
    record.updated_at_ms = now;
    record
}

pub fn stamp_updated(mut record: PlanRecord) -> PlanRecord {
    record.updated_at_ms = now_millis();
    record
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plan::{PlanTodo, PlanTodoStatus};
    use crate::infra::settings_store::test_support::with_temp_home;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_repo_id() -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        format!(
            "test-plan-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn sample_record(id: &str) -> PlanRecord {
        PlanRecord {
            id: id.to_string(),
            name: "Test Plan".into(),
            overview: "An overview.".into(),
            plan: "# Test Plan\n\nDo things.".into(),
            todos: vec![
                PlanTodo {
                    id: "step-1".into(),
                    content: "First step".into(),
                    status: PlanTodoStatus::InProgress,
                    note: None,
                },
                PlanTodo {
                    id: "step-2".into(),
                    content: "Second step".into(),
                    status: PlanTodoStatus::Pending,
                    note: None,
                },
            ],
            created_at_ms: 0,
            updated_at_ms: 0,
            chat_id: None,
            repo_root: None,
        }
    }

    #[test]
    fn save_get_list_delete_round_trip() {
        with_temp_home(|| {
            let repo_id = unique_repo_id();
            let record = stamp_new(sample_record("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"));
            save(&repo_id, &record).unwrap();

            let loaded = get(&repo_id, &record.id).unwrap();
            assert_eq!(loaded.name, "Test Plan");
            assert_eq!(loaded.todos.len(), 2);

            let listed = list(&repo_id).unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].id, record.id);
            assert_eq!(listed[0].todo_total, 2);

            delete(&repo_id, &record.id).unwrap();
            assert!(matches!(get(&repo_id, &record.id), Err(PlanError::NotFound(_))));

            let _ = fs::remove_dir_all(plan_dir(&repo_id).unwrap());
        });
    }

    #[test]
    fn rejects_path_traversal_ids() {
        let repo_id = unique_repo_id();
        assert!(matches!(
            get(&repo_id, "../evil"),
            Err(PlanError::Invalid(_))
        ));
    }
}
