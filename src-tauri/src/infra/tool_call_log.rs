//! Redacted, cross-project event-store log of every AI-harness tool call —
//! `~/.atlas/tool_calls.db`. Mirrors `infra::chat_store`/
//! `infra::git_action_log_store`'s shape: a fresh connection opened per
//! call (writes happen at most once per tool call, not a hot path relative
//! to a local SQLite insert), schema as a `CREATE TABLE IF NOT EXISTS`
//! constant, best-effort writes (a logging failure must never break a real
//! tool call — mirrors `infra::llm_debug_log::append`'s exact contract).
//!
//! Unlike `llm_debug_log` (off by default, logs the full raw request/
//! response including document text), this is **on by default**
//! (`LlmSettings.tool_call_logging`) but never stores raw document
//! content — `redact_args`/`redact_result` strip the handful of fields
//! that carry file text (see their doc comments) before anything reaches
//! this module, so what's persisted is safe to browse as an always-on
//! audit trail (tool name, path-shaped args, status, timing) without
//! leaking the documents themselves onto disk a second time.
//!
//! Retention is time-based, not count-based (unlike `git_action_log_store`'s
//! per-repo cap): every write opportunistically deletes rows older than
//! `RETENTION_DAYS`, no background job needed at this write volume.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, Row, ToSql};
use thiserror::Error;

use crate::domain::ai_access::ToolName;
use crate::domain::ai_tools::{ToolCall, ToolResult};
use crate::domain::tool_call_log::{ToolCallLogFilter, ToolCallLogPage, ToolCallLogRow};

const DB_FILE_NAME: &str = "tool_calls.db";
const RETENTION_DAYS: i64 = 30;

const SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 3000;

CREATE TABLE IF NOT EXISTS tool_calls (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  ts_ms         INTEGER NOT NULL,
  repo_root     TEXT NOT NULL,
  source        TEXT NOT NULL,
  round         INTEGER,
  provider_id   TEXT,
  model         TEXT,
  tool          TEXT NOT NULL,
  args_json     TEXT NOT NULL,
  status        TEXT NOT NULL,
  error_message TEXT,
  result_json   TEXT,
  duration_ms   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tool_calls_ts ON tool_calls(ts_ms DESC);
CREATE INDEX IF NOT EXISTS idx_tool_calls_repo ON tool_calls(repo_root, ts_ms DESC);
"#;

#[derive(Debug, Error)]
pub enum ToolCallLogError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings error: {0}")]
    Settings(#[from] crate::domain::settings::SettingsError),
}

fn db_path() -> Result<PathBuf, ToolCallLogError> {
    Ok(crate::infra::settings_store::settings_dir()?.join(DB_FILE_NAME))
}

fn open() -> Result<Connection, ToolCallLogError> {
    let path = db_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}

fn now_millis() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// One settled tool call, ready to persist — built by
/// `services::ai_tools::execute_tool_logged` from the `ToolCall`/
/// `Result<ToolResult, ToolError>` pair it wraps. `args_json`/`result_json`
/// must already be redacted (`redact_args`/`redact_result` below) — this
/// module never inspects tool semantics itself, only stores/queries.
#[derive(Debug, Clone)]
pub struct ToolCallLogEntry {
    pub repo_root: String,
    /// `"chat"` or `"standalone"` — see `ToolCallLogRow::source`.
    pub source: String,
    pub round: Option<u32>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub tool: String,
    pub args_json: serde_json::Value,
    /// `"ok"` or `"error"`.
    pub status: String,
    pub error_message: Option<String>,
    pub result_json: Option<serde_json::Value>,
    pub duration_ms: i64,
}

/// Best-effort — never propagates a failure into the tool call that
/// triggered it (disk full, permissions, home dir unavailable all silently
/// drop the entry), same contract as `infra::llm_debug_log::append`. No-op
/// entirely when `enabled` is `false`.
pub fn log_call(enabled: bool, entry: ToolCallLogEntry) {
    if !enabled {
        return;
    }
    let _ = try_log_call(entry);
}

fn try_log_call(entry: ToolCallLogEntry) -> Result<(), ToolCallLogError> {
    let conn = open()?;
    let now = now_millis();
    conn.execute(
        "INSERT INTO tool_calls
           (ts_ms, repo_root, source, round, provider_id, model, tool, args_json, status, error_message, result_json, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            now,
            entry.repo_root,
            entry.source,
            entry.round,
            entry.provider_id,
            entry.model,
            entry.tool,
            entry.args_json.to_string(),
            entry.status,
            entry.error_message,
            entry.result_json.map(|v| v.to_string()),
            entry.duration_ms,
        ],
    )?;

    let cutoff = now - RETENTION_DAYS * 24 * 60 * 60 * 1000;
    conn.execute("DELETE FROM tool_calls WHERE ts_ms < ?1", params![cutoff])?;
    Ok(())
}

fn row_to_log_row(row: &Row) -> rusqlite::Result<ToolCallLogRow> {
    let args_text: String = row.get(8)?;
    let result_text: Option<String> = row.get(11)?;
    Ok(ToolCallLogRow {
        id: row.get(0)?,
        ts_ms: row.get(1)?,
        repo_root: row.get(2)?,
        source: row.get(3)?,
        round: row.get(4)?,
        provider_id: row.get(5)?,
        model: row.get(6)?,
        tool: row.get(7)?,
        args_json: serde_json::from_str(&args_text).unwrap_or(serde_json::Value::Null),
        status: row.get(9)?,
        error_message: row.get(10)?,
        result_json: result_text.and_then(|t| serde_json::from_str(&t).ok()),
        duration_ms: row.get(12)?,
    })
}

const SELECT_COLUMNS: &str =
    "id, ts_ms, repo_root, source, round, provider_id, model, tool, args_json, status, error_message, result_json, duration_ms";

/// `filter`'s absent fields impose no constraint; `limit` is clamped to
/// `[1, 1000]` (default 200), `offset` to `>= 0`. Both are spliced into the
/// SQL text directly rather than bound as params — safe since they're
/// already-clamped integers, and it keeps the same bound-parameter list
/// shared verbatim between the count and the page query below.
fn build_where(filter: &ToolCallLogFilter) -> (String, Vec<Box<dyn ToSql>>) {
    let mut clauses: Vec<&str> = Vec::new();
    let mut values: Vec<Box<dyn ToSql>> = Vec::new();

    if let Some(repo_root) = &filter.repo_root {
        clauses.push("repo_root = ?");
        values.push(Box::new(repo_root.clone()));
    }
    if let Some(tool) = &filter.tool {
        clauses.push("tool = ?");
        values.push(Box::new(tool.clone()));
    }
    if let Some(status) = &filter.status {
        clauses.push("status = ?");
        values.push(Box::new(status.clone()));
    }
    if let Some(since_ms) = filter.since_ms {
        clauses.push("ts_ms >= ?");
        values.push(Box::new(since_ms));
    }
    if let Some(search) = &filter.search {
        clauses.push("(tool LIKE ? OR error_message LIKE ?)");
        let pattern = format!("%{search}%");
        values.push(Box::new(pattern.clone()));
        values.push(Box::new(pattern));
    }

    let where_sql = if clauses.is_empty() { String::new() } else { format!("WHERE {}", clauses.join(" AND ")) };
    (where_sql, values)
}

pub fn query(filter: &ToolCallLogFilter) -> Result<ToolCallLogPage, ToolCallLogError> {
    let conn = open()?;
    let (where_sql, values) = build_where(filter);
    let param_refs: Vec<&dyn ToSql> = values.iter().map(|v| v.as_ref()).collect();

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM tool_calls {where_sql}"),
        rusqlite::params_from_iter(param_refs.iter()),
        |row| row.get(0),
    )?;

    let limit = filter.limit.unwrap_or(200).clamp(1, 1000);
    let offset = filter.offset.unwrap_or(0).max(0);
    // `id DESC` as a tiebreaker: two tool calls can easily land in the same
    // millisecond (a `readFile`/`listFiles` pair a few microseconds apart),
    // and `ts_ms` alone leaves `ORDER BY` non-deterministic for ties —
    // `id` (autoincrement) is a strictly-increasing proxy for insert order.
    let select_sql = format!(
        "SELECT {SELECT_COLUMNS} FROM tool_calls {where_sql} ORDER BY ts_ms DESC, id DESC LIMIT {limit} OFFSET {offset}"
    );
    let mut stmt = conn.prepare(&select_sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(param_refs.iter()), row_to_log_row)?;
    let rows = rows.collect::<Result<Vec<_>, _>>()?;

    Ok(ToolCallLogPage { rows, total })
}

/// Deletes every row (`older_than_days: None`) or only rows older than
/// that many days. Returns the number of rows deleted.
pub fn clear(older_than_days: Option<u32>) -> Result<usize, ToolCallLogError> {
    let conn = open()?;
    let deleted = match older_than_days {
        Some(days) => {
            let cutoff = now_millis() - i64::from(days) * 24 * 60 * 60 * 1000;
            conn.execute("DELETE FROM tool_calls WHERE ts_ms < ?1", params![cutoff])?
        }
        None => conn.execute("DELETE FROM tool_calls", [])?,
    };
    Ok(deleted)
}

fn tool_name_str(name: ToolName) -> String {
    serde_json::to_value(name).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn redacted() -> serde_json::Value {
    serde_json::Value::String("<redacted>".to_string())
}

/// Redacted JSON view of a `ToolCall`'s `args` — the tag/args envelope
/// (`{"tool": "...", "args": {...}}`) is preserved so a logged entry looks
/// exactly like the wire shape everywhere else in this codebase, only the
/// fields that carry raw document text are replaced with a placeholder:
/// `writeFile.content`, `editFile.edits[].old`/`.new`,
/// `requestArtifact.prefill` (which can carry an example request body), and
/// `visualize.source` (a whole generated diagram).
/// Every other tool's args are already path/query/pattern-shaped, not
/// document content, so they pass through unchanged.
pub fn redact_args(call: &ToolCall) -> serde_json::Value {
    let mut value = serde_json::to_value(call).unwrap_or(serde_json::Value::Null);
    let Some(args) = value.get_mut("args").and_then(|v| v.as_object_mut()) else {
        return value;
    };
    match call {
        ToolCall::WriteFile(_) => {
            args.insert("content".to_string(), redacted());
        }
        ToolCall::EditFile(_) => {
            if let Some(edits) = args.get_mut("edits").and_then(|v| v.as_array_mut()) {
                for edit in edits {
                    if let Some(edit) = edit.as_object_mut() {
                        edit.insert("old".to_string(), redacted());
                        edit.insert("new".to_string(), redacted());
                    }
                }
            }
        }
        ToolCall::Memory(_) if args.contains_key("text") => {
            args.insert("text".to_string(), redacted());
        }
        ToolCall::CreatePlan(_) | ToolCall::UpdatePlan(_) if args.contains_key("plan") => {
            args.insert("plan".to_string(), redacted());
        }
        // `kind`/`title`/`purpose` are the identifying fields worth
        // auditing; `prefill` is a partial spec that can carry an example
        // payload, so it goes the way of `writeFile.content`.
        ToolCall::RequestArtifact(_) if args.contains_key("prefill") => {
            args.insert("prefill".to_string(), redacted());
        }
        // The diagram source is generated document-shaped text and can run
        // to hundreds of lines; `kind`/`format`/`title` are the fields
        // worth auditing.
        ToolCall::Visualize(_) if args.contains_key("source") => {
            args.insert("source".to_string(), redacted());
        }
        _ => {}
    }
    value
}

/// Redacted JSON view of a settled `ToolResult` — same envelope-preserving
/// approach as `redact_args`. Strips `file.content` (`readFile`),
/// `grepResults.matches[].text` (the matched source line), and
/// `diff.unifiedDiff` wherever a `FileDiffStats` appears (`gitDiff`,
/// `fileWritten`/`fileEdited`/`fileDeleted` — `linesAdded`/`linesRemoved`
/// are kept, since those are counts, not content), the `plan` body, and an
/// artifact's `content`/`rendered` (parameter tables and example payloads).
pub fn redact_result(result: &ToolResult) -> serde_json::Value {
    let mut value = serde_json::to_value(result).unwrap_or(serde_json::Value::Null);
    let Some(inner) = value.get_mut("result") else {
        return value;
    };
    match result {
        ToolResult::File { .. } => {
            if let Some(obj) = inner.as_object_mut() {
                obj.insert("content".to_string(), redacted());
            }
        }
        ToolResult::GrepResults { .. } => {
            if let Some(matches) = inner.get_mut("matches").and_then(|v| v.as_array_mut()) {
                for m in matches {
                    if let Some(m) = m.as_object_mut() {
                        m.insert("text".to_string(), redacted());
                        // Context lines are file content just as much as the
                        // matching line is — redacting only `text` would put
                        // the surrounding source straight into the log.
                        for key in ["before", "after"] {
                            if let Some(lines) = m.get_mut(key).and_then(|v| v.as_array_mut()) {
                                for line in lines {
                                    *line = redacted();
                                }
                            }
                        }
                    }
                }
            }
        }
        ToolResult::GitDiff { .. }
        | ToolResult::FileWritten { .. }
        | ToolResult::FileEdited { .. }
        | ToolResult::FileDeleted { .. } => {
            if let Some(diff) = inner.get_mut("diff").and_then(|v| v.as_object_mut()) {
                diff.insert("unifiedDiff".to_string(), redacted());
            }
        }
        ToolResult::PlanRead { .. } => {
            if let Some(obj) = inner.as_object_mut() {
                obj.insert("plan".to_string(), redacted());
            }
        }
        // The whole point of an artifact is the request/response detail it
        // carries — descriptions, example bodies — plus the AsciiDoc built
        // from it. Neither belongs in an always-on log; the id, kind, title
        // and status left behind are enough to audit that it was read.
        ToolResult::Artifact { .. } => {
            if let Some(artifact) = inner.get_mut("artifact").and_then(|v| v.as_object_mut()) {
                artifact.insert("content".to_string(), redacted());
            }
            if let Some(obj) = inner.as_object_mut() {
                obj.insert("rendered".to_string(), redacted());
            }
        }
        _ => {}
    }
    value
}

/// String label stored in `tool_calls.tool` — the same camelCase spelling
/// the wire protocol and settings UI already use for a `ToolName` (e.g.
/// `"writeFile"`), so a logged row's `tool` column matches what a user
/// would recognize from the approval-card/allowlist UI.
pub fn tool_label(call: &ToolCall) -> String {
    tool_name_str(call.name())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai_tools::{EditFileArgs, FileEdit, ReadFileArgs, WriteFileArgs};
    use crate::infra::settings_store::test_support::with_temp_home;

    fn sample_entry(repo_root: &str, tool: &str, status: &str) -> ToolCallLogEntry {
        ToolCallLogEntry {
            repo_root: repo_root.to_string(),
            source: "chat".to_string(),
            round: Some(1),
            provider_id: Some("alfagen".to_string()),
            model: Some("gpt-4o-mini".to_string()),
            tool: tool.to_string(),
            args_json: serde_json::json!({ "tool": tool, "args": { "path": "a.adoc" } }),
            status: status.to_string(),
            error_message: if status == "error" { Some("boom".to_string()) } else { None },
            result_json: if status == "ok" { Some(serde_json::json!({ "tool": "file" })) } else { None },
            duration_ms: 12,
        }
    }

    #[test]
    fn log_call_then_query_round_trips_an_entry() {
        with_temp_home(|| {
            log_call(true, sample_entry("/repo/one", "readFile", "ok"));

            let page = query(&ToolCallLogFilter::default()).unwrap();
            assert_eq!(page.total, 1);
            assert_eq!(page.rows[0].tool, "readFile");
            assert_eq!(page.rows[0].repo_root, "/repo/one");
            assert_eq!(page.rows[0].status, "ok");
            assert_eq!(page.rows[0].round, Some(1));
        });
    }

    #[test]
    fn log_call_is_a_no_op_when_disabled() {
        with_temp_home(|| {
            log_call(false, sample_entry("/repo/one", "readFile", "ok"));
            let page = query(&ToolCallLogFilter::default()).unwrap();
            assert_eq!(page.total, 0);
        });
    }

    #[test]
    fn query_filters_by_repo_root_tool_and_status() {
        with_temp_home(|| {
            log_call(true, sample_entry("/repo/one", "readFile", "ok"));
            log_call(true, sample_entry("/repo/one", "writeFile", "error"));
            log_call(true, sample_entry("/repo/two", "readFile", "ok"));

            let by_repo = query(&ToolCallLogFilter { repo_root: Some("/repo/one".to_string()), ..Default::default() }).unwrap();
            assert_eq!(by_repo.total, 2);

            let by_tool = query(&ToolCallLogFilter { tool: Some("writeFile".to_string()), ..Default::default() }).unwrap();
            assert_eq!(by_tool.total, 1);
            assert_eq!(by_tool.rows[0].status, "error");

            let by_status =
                query(&ToolCallLogFilter { status: Some("error".to_string()), ..Default::default() }).unwrap();
            assert_eq!(by_status.total, 1);
            assert_eq!(by_status.rows[0].error_message.as_deref(), Some("boom"));
        });
    }

    #[test]
    fn query_paginates_newest_first() {
        with_temp_home(|| {
            for i in 0..5 {
                log_call(true, sample_entry("/repo/one", &format!("tool{i}"), "ok"));
            }
            let page = query(&ToolCallLogFilter { limit: Some(2), offset: Some(1), ..Default::default() }).unwrap();
            assert_eq!(page.total, 5);
            assert_eq!(page.rows.len(), 2);
            assert_eq!(page.rows[0].tool, "tool3");
            assert_eq!(page.rows[1].tool, "tool2");
        });
    }

    #[test]
    fn clear_without_age_removes_everything() {
        with_temp_home(|| {
            log_call(true, sample_entry("/repo/one", "readFile", "ok"));
            let deleted = clear(None).unwrap();
            assert_eq!(deleted, 1);
            assert_eq!(query(&ToolCallLogFilter::default()).unwrap().total, 0);
        });
    }

    #[test]
    fn redact_args_strips_write_file_content_but_keeps_path() {
        let call = ToolCall::WriteFile(WriteFileArgs { path: "a.adoc".to_string(), content: "SECRET".to_string() });
        let redacted = redact_args(&call);
        assert_eq!(redacted["args"]["path"], "a.adoc");
        assert_eq!(redacted["args"]["content"], "<redacted>");
    }

    #[test]
    fn redact_args_strips_edit_file_old_and_new() {
        let call = ToolCall::EditFile(EditFileArgs {
            path: "a.adoc".to_string(),
            edits: vec![FileEdit { old: "SECRET-OLD".to_string(), new: "SECRET-NEW".to_string() }],
        });
        let redacted = redact_args(&call);
        assert_eq!(redacted["args"]["edits"][0]["old"], "<redacted>");
        assert_eq!(redacted["args"]["edits"][0]["new"], "<redacted>");
    }

    #[test]
    fn redact_args_passes_read_file_through_unchanged() {
        let call = ToolCall::ReadFile(ReadFileArgs { path: "a.adoc".to_string(), start_line: None, end_line: None,
    outline: None,
});
        let redacted = redact_args(&call);
        assert_eq!(redacted["args"]["path"], "a.adoc");
    }

    #[test]
    fn redact_result_strips_file_content_but_keeps_line_counts() {
        let result = ToolResult::File { content: "SECRET".to_string(), start_line: 1, end_line: 3, total_lines: 3 };
        let redacted = redact_result(&result);
        assert_eq!(redacted["result"]["content"], "<redacted>");
        assert_eq!(redacted["result"]["totalLines"], 3);
    }

    #[test]
    fn redact_result_strips_grep_match_text_including_context_lines() {
        let result = ToolResult::GrepResults {
            matches: vec![crate::domain::ai_tools::GrepMatch {
                path: "a.adoc".to_string(),
                line: 1,
                text: "SECRET".to_string(),
                before: vec!["ALSO SECRET".to_string()],
                after: vec!["SECRET TOO".to_string()],
            }],
            truncated: false,
        };
        let redacted = redact_result(&result);
        assert_eq!(redacted["result"]["matches"][0]["text"], "<redacted>");
        assert_eq!(redacted["result"]["matches"][0]["before"][0], "<redacted>");
        assert_eq!(redacted["result"]["matches"][0]["after"][0], "<redacted>");
        assert_eq!(redacted["result"]["matches"][0]["path"], "a.adoc");
    }

    #[test]
    fn redact_result_strips_unified_diff_but_keeps_line_counts() {
        let result = ToolResult::FileWritten {
            path: "a.adoc".to_string(),
            diff: crate::domain::ai_tools::FileDiffStats {
                lines_added: 3,
                lines_removed: 1,
                unified_diff: "SECRET DIFF".to_string(),
                truncated: false,
            },
            closed_macros: vec![],
        };
        let redacted = redact_result(&result);
        assert_eq!(redacted["result"]["diff"]["unifiedDiff"], "<redacted>");
        assert_eq!(redacted["result"]["diff"]["linesAdded"], 3);
    }

    #[test]
    fn redact_args_strips_memory_text_regardless_of_op() {
        let call = ToolCall::Memory(crate::domain::ai_tools::MemoryArgs {
            op: "note".to_string(),
            scope: "project".to_string(),
            text: Some("SECRET NOTE".to_string()),
            pattern: None,
            block: None,
            knob: None,
            part: None,
            snapshot_t: None,
        });
        let redacted = redact_args(&call);
        assert_eq!(redacted["args"]["text"], "<redacted>");
        assert_eq!(redacted["args"]["scope"], "project");
    }

    #[test]
    fn redact_args_strips_a_request_artifact_prefill_but_keeps_its_identity() {
        let call = ToolCall::RequestArtifact(crate::domain::ai_tools::RequestArtifactArgs {
            kind: crate::domain::artifact::ArtifactKind::HttpRequest,
            title: "Создание документа".to_string(),
            purpose: "Нужны входные параметры".to_string(),
            prefill: Some(crate::domain::artifact::ArtifactContent::HttpRequest(
                crate::domain::artifact::HttpRequestSpec {
                    method: "POST".to_string(),
                    ..Default::default()
                },
            )),
        });
        let redacted = redact_args(&call);
        assert_eq!(redacted["args"]["title"], "Создание документа");
        assert_eq!(redacted["args"]["kind"], "httpRequest");
        assert_eq!(redacted["args"]["prefill"], "<redacted>");
    }

    #[test]
    fn redact_result_strips_artifact_content_and_rendering() {
        let record = crate::domain::artifact::ArtifactRecord {
            id: "a1".to_string(),
            kind: crate::domain::artifact::ArtifactKind::HttpRequest,
            title: "T".to_string(),
            purpose: None,
            status: crate::domain::artifact::ArtifactStatus::Ready,
            content: crate::domain::artifact::ArtifactContent::HttpRequest(
                crate::domain::artifact::HttpRequestSpec {
                    method: "POST".to_string(),
                    ..Default::default()
                },
            ),
            created_at_ms: 1,
            updated_at_ms: 2,
            chat_id: None,
            repo_root: None,
        };
        let result = crate::services::ai_tools::artifact_result(record);
        let redacted = redact_result(&result);
        assert_eq!(redacted["result"]["artifact"]["id"], "a1");
        assert_eq!(redacted["result"]["artifact"]["status"], "ready");
        assert_eq!(redacted["result"]["artifact"]["content"], "<redacted>");
        assert_eq!(redacted["result"]["rendered"], "<redacted>");
    }

    #[test]
    fn tool_label_matches_the_wire_tag() {
        let call = ToolCall::ReadFile(ReadFileArgs { path: "a.adoc".to_string(), start_line: None, end_line: None,
    outline: None,
});
        assert_eq!(tool_label(&call), "readFile");
    }
}
