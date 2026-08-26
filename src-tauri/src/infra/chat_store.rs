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
//! return it byte-for-byte. `chats.todos`, by contrast, *is* typed
//! (`Vec<domain::ai_tools::Task>`) — unlike `MessageBlock`, `Task` is
//! already a stable, shared domain type used throughout the tool-calling
//! boundary, so there's no independent-evolution risk to guard against by
//! keeping it opaque too.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Row};
use thiserror::Error;

use crate::domain::ai_tools::Task;
use crate::domain::chat::{ChatSummary, LoadedChat};

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
  todos      TEXT NOT NULL DEFAULT '[]',
  active_plan_id TEXT,
  memory_extracted_ordinal INTEGER NOT NULL DEFAULT -1,
  pending_resume TEXT,
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
    migrate_add_todos_column(&conn)?;
    migrate_add_active_plan_id_column(&conn)?;
    migrate_add_memory_extracted_ordinal_column(&conn)?;
    migrate_add_pending_resume_column(&conn)?;
    Ok(conn)
}

/// Additive migration for a `chats` table created before this column
/// existed — `CREATE TABLE IF NOT EXISTS` above only shapes brand-new
/// databases. Checked-then-`ALTER` (SQLite has no `ADD COLUMN IF NOT
/// EXISTS`) rather than blindly running the `ALTER` and swallowing a
/// "duplicate column" error, which would also hide a real failure (a
/// locked db, say) behind "already migrated". Runs on every `open()` call
/// — cheap, one `PRAGMA table_info` query — since this store already
/// opens a fresh connection per call with no persistent place to remember
/// "already checked" (see this module's own doc comment). User data —
/// unlike `index_store`'s rebuildable embeddings cache, this must never be
/// wiped on a schema change.
fn migrate_add_todos_column(conn: &Connection) -> Result<(), ChatStoreError> {
    let mut stmt = conn.prepare("PRAGMA table_info(chats)")?;
    let has_todos = stmt
        .query_map([], |row| row.get::<_, String>(1))? // column 1 = name
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == "todos");
    if !has_todos {
        conn.execute("ALTER TABLE chats ADD COLUMN todos TEXT NOT NULL DEFAULT '[]'", [])?;
    }
    Ok(())
}

fn migrate_add_active_plan_id_column(conn: &Connection) -> Result<(), ChatStoreError> {
    let mut stmt = conn.prepare("PRAGMA table_info(chats)")?;
    let has_col = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == "active_plan_id");
    if !has_col {
        conn.execute("ALTER TABLE chats ADD COLUMN active_plan_id TEXT", [])?;
    }
    Ok(())
}

fn migrate_add_memory_extracted_ordinal_column(conn: &Connection) -> Result<(), ChatStoreError> {
    let mut stmt = conn.prepare("PRAGMA table_info(chats)")?;
    let has_col = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == "memory_extracted_ordinal");
    if !has_col {
        conn.execute(
            "ALTER TABLE chats ADD COLUMN memory_extracted_ordinal INTEGER NOT NULL DEFAULT -1",
            [],
        )?;
    }
    Ok(())
}

/// Opaque JSON blob of the frontend's `PendingApproval` — set when a turn
/// pauses awaiting a tool-approval/`askUser` decision (before the turn as a
/// whole has settled), cleared (`NULL`) once it resolves. Lets a chat
/// reopened after a full app restart (not just a panel close within one
/// running session) restore enough state (`history`/`round`/`budgetUsed`)
/// to resume via `llm_chat_stream_resume` — see `commands::chat_history`.
fn migrate_add_pending_resume_column(conn: &Connection) -> Result<(), ChatStoreError> {
    let mut stmt = conn.prepare("PRAGMA table_info(chats)")?;
    let has_col = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == "pending_resume");
    if !has_col {
        conn.execute("ALTER TABLE chats ADD COLUMN pending_resume TEXT", [])?;
    }
    Ok(())
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

/// One chat's full state: its messages (opaque JSON, save order — each
/// element exactly as `save_chat` received it) and its todo checklist.
pub fn load_chat(chat_id: &str) -> Result<LoadedChat, ChatStoreError> {
    let conn = open()?;
    let mut stmt = conn.prepare("SELECT data FROM messages WHERE chat_id = ?1 ORDER BY ordinal ASC")?;
    let raw: Vec<String> = stmt
        .query_map(params![chat_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let messages = raw
        .iter()
        .map(|s| serde_json::from_str(s).map_err(ChatStoreError::from))
        .collect::<Result<Vec<_>, _>>()?;

    // No `chats` row (nothing saved yet for this id) yields empty
    // messages/todos rather than `NotFound` — keeps this function total
    // for a caller like `useChatHistory::switchChat`, which only ever
    // passes ids it already knows exist, but there's no reason to make
    // this partial for that.
    let todos_json: Option<String> = conn
        .query_row("SELECT todos FROM chats WHERE id = ?1", params![chat_id], |row| row.get(0))
        .optional()?;
    let todos = match todos_json {
        Some(s) => serde_json::from_str(&s)?,
        None => Vec::new(),
    };

    let active_plan_id: Option<String> = conn
        .query_row(
            "SELECT active_plan_id FROM chats WHERE id = ?1",
            params![chat_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();

    let pending_resume_json: Option<String> = conn
        .query_row(
            "SELECT pending_resume FROM chats WHERE id = ?1",
            params![chat_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let pending_resume = pending_resume_json
        .map(|s| serde_json::from_str(&s))
        .transpose()?;

    Ok(LoadedChat {
        messages,
        todos,
        active_plan_id,
        pending_resume,
    })
}

/// Upserts the `chats` row (title/`todos`/`updated_at` always overwritten;
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
    todos: &[Task],
    active_plan_id: Option<&str>,
    pending_resume: Option<&serde_json::Value>,
) -> Result<ChatSummary, ChatStoreError> {
    let mut conn = open()?;
    let now = now_millis();
    let todos_json = serde_json::to_string(todos)?;
    let pending_resume_json = pending_resume.map(serde_json::to_string).transpose()?;
    let tx = conn.transaction()?;

    tx.execute(
        "INSERT INTO chats (id, repo_root, title, todos, active_plan_id, pending_resume, archived, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?7)
         ON CONFLICT(id) DO UPDATE SET
           title = excluded.title,
           todos = excluded.todos,
           active_plan_id = excluded.active_plan_id,
           pending_resume = excluded.pending_resume,
           updated_at = excluded.updated_at",
        params![chat_id, repo_root, title, todos_json, active_plan_id, pending_resume_json, now],
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

/// Last message ordinal the memory pipeline has already processed for this
/// chat (`-1` = never). Missing chat → `NotFound`.
pub fn memory_extracted_ordinal(chat_id: &str) -> Result<i64, ChatStoreError> {
    let conn = open()?;
    conn.query_row(
        "SELECT memory_extracted_ordinal FROM chats WHERE id = ?1",
        params![chat_id],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| ChatStoreError::NotFound(chat_id.to_string()))
}

pub fn set_memory_extracted_ordinal(chat_id: &str, ordinal: i64) -> Result<(), ChatStoreError> {
    let conn = open()?;
    let changed = conn.execute(
        "UPDATE chats SET memory_extracted_ordinal = ?1 WHERE id = ?2",
        params![ordinal, chat_id],
    )?;
    if changed == 0 {
        return Err(ChatStoreError::NotFound(chat_id.to_string()));
    }
    Ok(())
}

/// Repo root stored on the chat row — used by the memory pipeline to refuse
/// writing OptMem for a different project than the one that owns the chat.
pub fn chat_repo_root(chat_id: &str) -> Result<String, ChatStoreError> {
    let conn = open()?;
    conn.query_row(
        "SELECT repo_root FROM chats WHERE id = ?1",
        params![chat_id],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| ChatStoreError::NotFound(chat_id.to_string()))
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
    use crate::domain::ai_tools::TodoStatus;
    use crate::infra::settings_store::test_support::with_temp_home;

    fn sample_message(text: &str) -> serde_json::Value {
        serde_json::json!({ "id": text, "role": "user", "content": text })
    }

    fn sample_todo(id: &str, title: &str) -> Task {
        Task { id: id.to_string(), title: title.to_string(), status: TodoStatus::Pending, note: None }
    }

    #[test]
    fn save_then_list_round_trips_a_new_chat() {
        with_temp_home(|| {
            let repo = "/repo/one";
            let summary = save_chat(repo, "chat-1", "Первый вопрос", &[sample_message("hi")], &[], None, None).unwrap();
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
            save_chat("/repo/one", "chat-1", "t", &messages, &[], None, None).unwrap();

            let loaded = load_chat("chat-1").unwrap();
            assert_eq!(loaded.messages, messages);
        });
    }

    #[test]
    fn resaving_a_chat_replaces_its_messages_and_preserves_created_at() {
        with_temp_home(|| {
            let first = save_chat("/repo/one", "chat-1", "t", &[sample_message("a")], &[], None, None).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
            let second =
                save_chat("/repo/one", "chat-1", "t2", &[sample_message("a"), sample_message("b")], &[], None, None).unwrap();

            assert_eq!(second.created_at, first.created_at);
            assert!(second.updated_at >= first.updated_at);
            assert_eq!(second.title, "t2");
            assert_eq!(load_chat("chat-1").unwrap().messages.len(), 2);

            // Still exactly one row in the active list, not a duplicate.
            assert_eq!(list_chats("/repo/one", false).unwrap().len(), 1);
        });
    }

    #[test]
    fn archiving_moves_a_chat_between_active_and_archived_lists() {
        with_temp_home(|| {
            let repo = "/repo/one";
            save_chat(repo, "chat-1", "t", &[sample_message("a")], &[], None, None).unwrap();

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
            save_chat("/repo/one", "chat-1", "t", &[sample_message("a")], &[], None, None).unwrap();
            save_chat("/repo/two", "chat-2", "t", &[sample_message("a")], &[], None, None).unwrap();

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
            save_chat(repo, "chat-1", "t", &[sample_message("a")], &[], None, None).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
            save_chat(repo, "chat-2", "t", &[sample_message("a")], &[], None, None).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
            save_chat(repo, "chat-1", "t", &[sample_message("a"), sample_message("b")], &[], None, None).unwrap();

            let active = list_chats(repo, false).unwrap();
            assert_eq!(active.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["chat-1", "chat-2"]);
        });
    }

    #[test]
    fn deleting_messages_of_a_missing_chat_is_a_harmless_no_op() {
        with_temp_home(|| {
            // save_chat on a brand-new id: the DELETE before insert matches
            // zero rows, which must not error.
            let summary = save_chat("/repo/one", "chat-new", "t", &[], &[], None, None).unwrap();
            assert_eq!(summary.id, "chat-new");
            assert!(load_chat("chat-new").unwrap().messages.is_empty());
        });
    }

    #[test]
    fn save_then_load_todos_round_trips() {
        with_temp_home(|| {
            let todos = vec![sample_todo("t1", "Write the docs")];
            save_chat("/repo/one", "chat-1", "t", &[sample_message("a")], &todos, None, None).unwrap();
            assert_eq!(load_chat("chat-1").unwrap().todos, todos);
        });
    }

    #[test]
    fn resaving_a_chat_replaces_its_todos() {
        with_temp_home(|| {
            save_chat("/repo/one", "chat-1", "t", &[sample_message("a")], &[sample_todo("t1", "first")], None, None).unwrap();
            save_chat("/repo/one", "chat-1", "t", &[sample_message("a")], &[sample_todo("t2", "second")], None, None).unwrap();
            assert_eq!(load_chat("chat-1").unwrap().todos, vec![sample_todo("t2", "second")]);
        });
    }

    /// Simulates a database created before this feature: a raw connection
    /// at the same path, with the legacy `chats` schema (no `todos`
    /// column), closed *before* this module's own `open()` ever runs
    /// against this path — then exercises the normal `load_chat`/
    /// `save_chat` API and confirms the migration happens transparently,
    /// with the pre-existing row untouched.
    #[test]
    fn opening_a_pre_existing_database_without_the_todos_column_migrates_cleanly() {
        with_temp_home(|| {
            let path = db_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            {
                let legacy = Connection::open(&path).unwrap();
                legacy
                    .execute_batch(
                        "CREATE TABLE chats (
                           id TEXT PRIMARY KEY, repo_root TEXT NOT NULL, title TEXT NOT NULL DEFAULT '',
                           archived INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
                         );
                         CREATE TABLE messages (
                           chat_id TEXT NOT NULL, ordinal INTEGER NOT NULL, data TEXT NOT NULL,
                           PRIMARY KEY (chat_id, ordinal)
                         );
                         INSERT INTO chats (id, repo_root, title, archived, created_at, updated_at)
                           VALUES ('chat-old', '/repo/one', 'old chat', 0, 1, 1);",
                    )
                    .unwrap();
            }

            let loaded = load_chat("chat-old").unwrap();
            assert!(loaded.todos.is_empty());

            let updated =
                save_chat(
                    "/repo/one",
                    "chat-old",
                    "old chat",
                    &[sample_message("a")],
                    &[sample_todo("t1", "x")],
                    None,
                    None,
                )
                .unwrap();
            assert_eq!(updated.created_at, 1); // preserved across migration + upsert
            assert_eq!(load_chat("chat-old").unwrap().todos, vec![sample_todo("t1", "x")]);
        });
    }

    #[test]
    fn memory_extracted_ordinal_defaults_to_minus_one_and_survives_resave() {
        with_temp_home(|| {
            save_chat("/repo/one", "chat-1", "t", &[sample_message("a")], &[], None, None).unwrap();
            assert_eq!(memory_extracted_ordinal("chat-1").unwrap(), -1);
            set_memory_extracted_ordinal("chat-1", 0).unwrap();
            save_chat(
                "/repo/one",
                "chat-1",
                "t",
                &[sample_message("a"), sample_message("b")],
                &[],
                None,
                None,
            )
            .unwrap();
            assert_eq!(memory_extracted_ordinal("chat-1").unwrap(), 0);
            assert_eq!(chat_repo_root("chat-1").unwrap(), "/repo/one");
        });
    }

    #[test]
    fn opening_a_pre_existing_database_without_memory_extracted_ordinal_migrates() {
        with_temp_home(|| {
            let path = db_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            {
                let legacy = Connection::open(&path).unwrap();
                legacy
                    .execute_batch(
                        "CREATE TABLE chats (
                           id TEXT PRIMARY KEY, repo_root TEXT NOT NULL, title TEXT NOT NULL DEFAULT '',
                           archived INTEGER NOT NULL DEFAULT 0, todos TEXT NOT NULL DEFAULT '[]',
                           created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
                         );
                         CREATE TABLE messages (
                           chat_id TEXT NOT NULL, ordinal INTEGER NOT NULL, data TEXT NOT NULL,
                           PRIMARY KEY (chat_id, ordinal)
                         );
                         INSERT INTO chats (id, repo_root, title, archived, todos, created_at, updated_at)
                           VALUES ('chat-old', '/repo/one', 'old chat', 0, '[]', 1, 1);",
                    )
                    .unwrap();
            }
            assert_eq!(memory_extracted_ordinal("chat-old").unwrap(), -1);
        });
    }

    #[test]
    fn save_then_load_pending_resume_round_trips() {
        with_temp_home(|| {
            let pending = serde_json::json!({"round": 2, "budgetUsed": 5, "calls": []});
            save_chat("/repo/one", "chat-1", "t", &[sample_message("a")], &[], None, Some(&pending)).unwrap();
            assert_eq!(load_chat("chat-1").unwrap().pending_resume, Some(pending));
        });
    }

    #[test]
    fn resaving_without_pending_resume_clears_it() {
        with_temp_home(|| {
            let pending = serde_json::json!({"round": 1});
            save_chat("/repo/one", "chat-1", "t", &[sample_message("a")], &[], None, Some(&pending)).unwrap();
            assert!(load_chat("chat-1").unwrap().pending_resume.is_some());

            save_chat("/repo/one", "chat-1", "t", &[sample_message("a")], &[], None, None).unwrap();
            assert_eq!(load_chat("chat-1").unwrap().pending_resume, None);
        });
    }

    #[test]
    fn opening_a_pre_existing_database_without_pending_resume_column_migrates_cleanly() {
        with_temp_home(|| {
            let path = db_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            {
                let legacy = Connection::open(&path).unwrap();
                legacy
                    .execute_batch(
                        "CREATE TABLE chats (
                           id TEXT PRIMARY KEY, repo_root TEXT NOT NULL, title TEXT NOT NULL DEFAULT '',
                           archived INTEGER NOT NULL DEFAULT 0, todos TEXT NOT NULL DEFAULT '[]',
                           created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
                         );
                         CREATE TABLE messages (
                           chat_id TEXT NOT NULL, ordinal INTEGER NOT NULL, data TEXT NOT NULL,
                           PRIMARY KEY (chat_id, ordinal)
                         );
                         INSERT INTO chats (id, repo_root, title, archived, todos, created_at, updated_at)
                           VALUES ('chat-old', '/repo/one', 'old chat', 0, '[]', 1, 1);",
                    )
                    .unwrap();
            }
            assert_eq!(load_chat("chat-old").unwrap().pending_resume, None);

            let pending = serde_json::json!({"round": 3});
            save_chat("/repo/one", "chat-old", "old chat", &[sample_message("a")], &[], None, Some(&pending)).unwrap();
            assert_eq!(load_chat("chat-old").unwrap().pending_resume, Some(pending));
        });
    }
}
