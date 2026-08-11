//! Persisted, cross-project log of user-initiated git actions — `~/.atlas/git_action_log.db`.
//! Mirrors `infra::chat_store`'s shape exactly: a fresh connection opened
//! per call (writes happen at most once per user action, not a hot path),
//! schema as a `CREATE TABLE IF NOT EXISTS` constant, `payload` stored as
//! an opaque JSON blob this module never parses (see `domain::git_action_log`'s
//! module doc). Rows are scoped by `repo_root` like `chats`, not per-project
//! like `infra::index_store`'s rebuildable cache — this is user history,
//! kept in one shared file across projects.

use std::path::PathBuf;

use rusqlite::{params, Connection};
use thiserror::Error;

use crate::domain::git_action_log::GitActionLogEntry;

const DB_FILE_NAME: &str = "git_action_log.db";

/// Rows kept per `repo_root` — old entries beyond this are pruned on every
/// `append_entry` call. This is a UI activity trail, not a durable audit
/// log, so unbounded growth isn't warranted the way it is for `chats`.
const MAX_ENTRIES_PER_REPO: i64 = 50;

const SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 3000;

CREATE TABLE IF NOT EXISTS git_action_log (
  id         TEXT PRIMARY KEY,
  repo_root  TEXT NOT NULL,
  kind       TEXT NOT NULL,
  summary    TEXT NOT NULL,
  payload    TEXT NOT NULL,
  undoable   INTEGER NOT NULL,
  undone     INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_git_action_log_repo_root ON git_action_log(repo_root, created_at DESC);
"#;

#[derive(Debug, Error)]
pub enum GitActionLogStoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings error: {0}")]
    Settings(#[from] crate::domain::settings::SettingsError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("log entry not found: {0}")]
    NotFound(String),
}

fn db_path() -> Result<PathBuf, GitActionLogStoreError> {
    Ok(crate::infra::settings_store::settings_dir()?.join(DB_FILE_NAME))
}

fn open() -> Result<Connection, GitActionLogStoreError> {
    let path = db_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<GitActionLogEntry> {
    let payload_json: String = row.get(3)?;
    let payload = serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null);
    Ok(GitActionLogEntry {
        id: row.get(0)?,
        kind: row.get(1)?,
        summary: row.get(2)?,
        payload,
        undoable: row.get::<_, i64>(4)? != 0,
        undone: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
    })
}

/// Most recent entries for one repository, newest first.
pub fn list_entries(
    repo_root: &str,
    limit: i64,
) -> Result<Vec<GitActionLogEntry>, GitActionLogStoreError> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT id, kind, summary, payload, undoable, undone, created_at
         FROM git_action_log WHERE repo_root = ?1
         ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![repo_root, limit], row_to_entry)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(GitActionLogStoreError::from)
}

/// Inserts one entry, then prunes rows for `repo_root` beyond
/// `MAX_ENTRIES_PER_REPO` (oldest first).
pub fn append_entry(
    repo_root: &str,
    entry: &GitActionLogEntry,
) -> Result<(), GitActionLogStoreError> {
    let mut conn = open()?;
    let payload_json = serde_json::to_string(&entry.payload)?;
    let tx = conn.transaction()?;

    tx.execute(
        "INSERT INTO git_action_log (id, repo_root, kind, summary, payload, undoable, undone, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            entry.id,
            repo_root,
            entry.kind,
            entry.summary,
            payload_json,
            entry.undoable as i64,
            entry.undone as i64,
            entry.created_at,
        ],
    )?;

    tx.execute(
        "DELETE FROM git_action_log WHERE repo_root = ?1 AND id NOT IN (
           SELECT id FROM git_action_log WHERE repo_root = ?1
           ORDER BY created_at DESC LIMIT ?2
         )",
        params![repo_root, MAX_ENTRIES_PER_REPO],
    )?;

    tx.commit()?;
    Ok(())
}

pub fn mark_undone(id: &str) -> Result<(), GitActionLogStoreError> {
    let conn = open()?;
    let changed = conn.execute(
        "UPDATE git_action_log SET undone = 1 WHERE id = ?1",
        params![id],
    )?;
    if changed == 0 {
        return Err(GitActionLogStoreError::NotFound(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;

    fn sample_entry(id: &str, created_at: i64) -> GitActionLogEntry {
        GitActionLogEntry {
            id: id.to_string(),
            kind: "stage".to_string(),
            summary: format!("Added to stage ({id})"),
            payload: serde_json::json!({ "kind": "stage", "paths": ["a.txt"] }),
            undoable: true,
            undone: false,
            created_at,
        }
    }

    #[test]
    fn append_then_list_round_trips_an_entry() {
        with_temp_home(|| {
            let repo = "/repo/one";
            append_entry(repo, &sample_entry("e1", 1000)).unwrap();

            let entries = list_entries(repo, 50).unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].id, "e1");
            assert_eq!(entries[0].kind, "stage");
            assert!(entries[0].undoable);
            assert!(!entries[0].undone);
            assert_eq!(entries[0].payload["paths"][0], "a.txt");
        });
    }

    #[test]
    fn list_entries_scoped_to_repo_root_and_ordered_newest_first() {
        with_temp_home(|| {
            append_entry("/repo/a", &sample_entry("a1", 1000)).unwrap();
            append_entry("/repo/a", &sample_entry("a2", 2000)).unwrap();
            append_entry("/repo/b", &sample_entry("b1", 1500)).unwrap();

            let entries_a = list_entries("/repo/a", 50).unwrap();
            assert_eq!(entries_a.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["a2", "a1"]);

            let entries_b = list_entries("/repo/b", 50).unwrap();
            assert_eq!(entries_b.len(), 1);
            assert_eq!(entries_b[0].id, "b1");
        });
    }

    #[test]
    fn mark_undone_flips_the_flag() {
        with_temp_home(|| {
            let repo = "/repo/one";
            append_entry(repo, &sample_entry("e1", 1000)).unwrap();
            mark_undone("e1").unwrap();

            let entries = list_entries(repo, 50).unwrap();
            assert!(entries[0].undone);
        });
    }

    #[test]
    fn mark_undone_unknown_id_errors() {
        with_temp_home(|| {
            assert!(matches!(mark_undone("nope"), Err(GitActionLogStoreError::NotFound(_))));
        });
    }

    #[test]
    fn append_prunes_beyond_max_entries_per_repo() {
        with_temp_home(|| {
            let repo = "/repo/one";
            for i in 0..(MAX_ENTRIES_PER_REPO + 5) {
                append_entry(repo, &sample_entry(&format!("e{i}"), 1000 + i)).unwrap();
            }
            let entries = list_entries(repo, 200).unwrap();
            assert_eq!(entries.len() as i64, MAX_ENTRIES_PER_REPO);
            // Newest survive: the last-inserted id should still be present,
            // the earliest ones should have been pruned.
            assert!(entries.iter().any(|e| e.id == format!("e{}", MAX_ENTRIES_PER_REPO + 4)));
            assert!(!entries.iter().any(|e| e.id == "e0"));
        });
    }
}
