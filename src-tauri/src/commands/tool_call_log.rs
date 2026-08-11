//! Tauri commands for the persisted tool-call log — thin wrappers over
//! `infra::tool_call_log`, no services layer (mirrors
//! `commands::chat_history`/`commands::git_action_log`, which call their own
//! infra store directly for the same reason: lightweight local SQLite I/O,
//! not network calls). Plain sync `fn`s, same precedent.

use crate::domain::tool_call_log::{ToolCallLogFilter, ToolCallLogPage};
use crate::infra::tool_call_log;

#[tauri::command]
pub fn tool_call_log_query(filter: ToolCallLogFilter) -> Result<ToolCallLogPage, String> {
    tool_call_log::query(&filter).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn tool_call_log_clear(older_than_days: Option<u32>) -> Result<usize, String> {
    tool_call_log::clear(older_than_days).map_err(|e| e.to_string())
}
