//! Global, cross-project store for assistant chat history — `~/.atlas/chat.db`.
//! Unlike `infra::index_store::IndexStore` (per-project, held open as a
//! long-lived `Mutex<Connection>` because it's queried frequently during
//! search), this opens a fresh connection per call: chat saves/loads happen
//! at most once per completed turn or chat switch, matching the frequency
//! every *other* global store in this codebase already assumes (all of
//! them JSON-file-per-call, no managed state) — the difference here is
//! only the file format. `PRAGMA busy_timeout` (not needed by `IndexStore`,
//! which never contends with itself) covers the extra transient-lock risk
//! that open-per-call introduces versus one long-lived connection.
//!
//! `messages.data` is an opaque JSON blob — this module never parses a
//! message's internal shape (the frontend's `ChatMessage`/`MessageBlock`
//! union, which evolves independently). Rust's only job is to store and
//! return it byte-for-byte.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Row};
use thiserror::Error;

use crate::domain::chat::ChatSummary;

const DB_FILE_NAME: &str = "chat.db";

const SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 3000;

CREATE TABLE IF NOT EXISTS chats (
  id         TEXT PRIMARY KEY,
  repo_root  TEXT NOT NULL,
  title      TEXT NOT NULL DEFAULT '',
  archived   INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_chats_repo_root ON chats(repo_root, archived, updated_at DESC);

CREATE TABLE IF NOT EXISTS messages (
  chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  data    TEXT NOT NULL,
  PRIMARY KEY (chat_id, ordinal)
);
"#;

#[derive(Debug, Error)]
pub enum ChatStoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings error: {0}")]
    Settings(#[from] crate::domain::settings::SettingsError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("chat not found: {0}")]
    NotFound(String),
}

fn db_path() -> Result<PathBuf, ChatStoreError> {
    Ok(crate::infra::settings_store::settings_dir()?.join(DB_FILE_NAME))
}

fn open() -> Result<Connection, ChatStoreError> {
    let path = db_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn row_to_summary(row: &Row) -> rusqlite::Result<ChatSummary> {
    Ok(ChatSummary {
        id: row.get(0)?,
        repo_root: row.get(1)?,
        title: row.get(2)?,
        archived: row.get::<_, i64>(3)? != 0,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn load_summary(conn: &Connection, chat_id: &str) -> Result<Option<ChatSummary>, ChatStoreError> {
    conn.query_row(
        "SELECT id, repo_root, title, archived, created_at, updated_at FROM chats WHERE id = ?1",
        params![chat_id],
        row_to_summary,
    )
    .optional()
    .map_err(ChatStoreError::from)
}

/// Active or archived chats for one repository, most recently updated
/// first — `chats[0]` (when `archived == false`) is what a caller should
/// auto-load as "the last active chat".
pub fn list_chats(repo_root: &str, archived: bool) -> Result<Vec<ChatSummary>, ChatStoreError> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT id, repo_root, title, archived, created_at, updated_at
         FROM chats WHERE repo_root = ?1 AND archived = ?2
         ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map(params![repo_root, archived as i64], row_to_summary)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(ChatStoreError::from)
}

/// One chat's messages, in save order — each element is the opaque
/// `ChatMessage` JSON blob exactly as `save_chat` received it.
pub fn load_messages(chat_id: &str) -> Result<Vec<serde_json::Value>, ChatStoreError> {
    let conn = open()?;
    let mut stmt = conn.prepare("SELECT data FROM messages WHERE chat_id = ?1 ORDER BY ordinal ASC")?;
    let raw: Vec<String> = stmt
        .query_map(params![chat_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    raw.iter().map(|s| serde_json::from_str(s).map_err(ChatStoreError::from)).collect()
}

/// Upserts the `chats` row (title/`updated_at` always overwritten;
/// `created_at`/`archived` preserved across an existing row) and replaces
/// `messages` wholesale — delete-then-reinsert under one transaction,
/// mirroring `IndexStore::replace_chunks_for_file`'s established pattern.
/// Sound at conversation-sized message counts; the O(total messages) cost
/// per save is exactly the tradeoff auto-compaction would address, and
/// that's out of scope for this feature.
pub fn save_chat(
    repo_root: &str,
    chat_id: &str,
    title: &str,
    messages: &[serde_json::Value],
) -> Result<ChatSummary, ChatStoreError> {
    let mut conn = open()?;
    let now = now_millis();
    let tx = conn.transaction()?;

    tx.execute(
        "INSERT INTO chats (id, repo_root, title, archived, created_at, updated_at)
         VALUES (?1, ?2, ?3, 0, ?4, ?4)
         ON CONFLICT(id) DO UPDATE SET title = excluded.title, updated_at = excluded.updated_at",
        params![chat_id, repo_root, title, now],
    )?;

    tx.execute("DELETE FROM messages WHERE chat_id = ?1", params![chat_id])?;
    for (ordinal, message) in messages.iter().enumerate() {
        let data = serde_json::to_string(message)?;
        tx.execute(
            "INSERT INTO messages (chat_id, ordinal, data) VALUES (?1, ?2, ?3)",
            params![chat_id, ordinal as i64, data],
        )?;
    }
    tx.commit()?;

    load_summary(&conn, chat_id)?.ok_or_else(|| ChatStoreError::NotFound(chat_id.to_string()))
}

pub fn set_archived(chat_id: &str, archived: bool) -> Result<(), ChatStoreError> {
    let conn = open()?;
    let changed = conn.execute(
        "UPDATE chats SET archived = ?1 WHERE id = ?2",
        params![archived as i64, chat_id],
    )?;
    if changed == 0 {
        return Err(ChatStoreError::NotFound(chat_id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;

    fn sample_message(text: &str) -> serde_json::Value {
        serde_json::json!({ "id": text, "role": "user", "content": text })
    }

    #[test]
    fn save_then_list_round_trips_a_new_chat() {
        with_temp_home(|| {
            let repo = "/repo/one";
            let summary = save_chat(repo, "chat-1", "Первый вопрос", &[sample_message("hi")]).unwrap();
            assert_eq!(summary.id, "chat-1");
            assert_eq!(summary.repo_root, repo);
            assert_eq!(summary.title, "Первый вопрос");
            assert!(!summary.archived);

            let active = list_chats(repo, false).unwrap();
            assert_eq!(active.len(), 1);
            assert_eq!(active[0].id, "chat-1");
        });
    }

    #[test]
    fn save_then_load_messages_preserves_order() {
        with_temp_home(|| {
            let messages = vec![sample_message("first"), sample_message("second"), sample_message("third")];
            save_chat("/repo/one", "chat-1", "t", &messages).unwrap();

            let loaded = load_messages("chat-1").unwrap();
            assert_eq!(loaded, messages);
        });
    }

    #[test]
    fn resaving_a_chat_replaces_its_messages_and_preserves_created_at() {
        with_temp_home(|| {
            let first = save_chat("/repo/one", "chat-1", "t", &[sample_message("a")]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
            let second = save_chat("/repo/one", "chat-1", "t2", &[sample_message("a"), sample_message("b")]).unwrap();

            assert_eq!(second.created_at, first.created_at);
            assert!(second.updated_at >= first.updated_at);
            assert_eq!(second.title, "t2");
            assert_eq!(load_messages("chat-1").unwrap().len(), 2);

            // Still exactly one row in the active list, not a duplicate.
            assert_eq!(list_chats("/repo/one", false).unwrap().len(), 1);
        });
    }

    #[test]
    fn archiving_moves_a_chat_between_active_and_archived_lists() {
        with_temp_home(|| {
            let repo = "/repo/one";
            save_chat(repo, "chat-1", "t", &[sample_message("a")]).unwrap();

            set_archived("chat-1", true).unwrap();
            assert!(list_chats(repo, false).unwrap().is_empty());
            assert_eq!(list_chats(repo, true).unwrap().len(), 1);

            set_archived("chat-1", false).unwrap();
            assert_eq!(list_chats(repo, false).unwrap().len(), 1);
            assert!(list_chats(repo, true).unwrap().is_empty());
        });
    }

    #[test]
    fn set_archived_on_an_unknown_chat_fails() {
        with_temp_home(|| {
            let err = set_archived("does-not-exist", true).unwrap_err();
            assert!(matches!(err, ChatStoreError::NotFound(_)));
        });
    }

    #[test]
    fn chats_are_scoped_to_their_repo_root() {
        with_temp_home(|| {
            save_chat("/repo/one", "chat-1", "t", &[sample_message("a")]).unwrap();
            save_chat("/repo/two", "chat-2", "t", &[sample_message("a")]).unwrap();

            let repo_one = list_chats("/repo/one", false).unwrap();
            assert_eq!(repo_one.len(), 1);
            assert_eq!(repo_one[0].id, "chat-1");

            let repo_two = list_chats("/repo/two", false).unwrap();
            assert_eq!(repo_two.len(), 1);
            assert_eq!(repo_two[0].id, "chat-2");
        });
    }

    #[test]
    fn list_chats_orders_by_most_recently_updated_first() {
        with_temp_home(|| {
            let repo = "/repo/one";
            save_chat(repo, "chat-1", "t", &[sample_message("a")]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
            save_chat(repo, "chat-2", "t", &[sample_message("a")]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
            save_chat(repo, "chat-1", "t", &[sample_message("a"), sample_message("b")]).unwrap();

            let active = list_chats(repo, false).unwrap();
            assert_eq!(active.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["chat-1", "chat-2"]);
        });
    }

    #[test]
    fn deleting_messages_of_a_missing_chat_is_a_harmless_no_op() {
        with_temp_home(|| {
            // save_chat on a brand-new id: the DELETE before insert matches
            // zero rows, which must not error.
            let summary = save_chat("/repo/one", "chat-new", "t", &[]).unwrap();
            assert_eq!(summary.id, "chat-new");
            assert!(load_messages("chat-new").unwrap().is_empty());
        });
    }
}
