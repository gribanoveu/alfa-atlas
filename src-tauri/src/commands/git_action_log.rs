//! Tauri commands for the persisted git action log — thin wrappers over
//! `infra::git_action_log_store`, no services layer (mirrors
//! `commands::chat_history`, which calls its own infra store directly for
//! the same reason: lightweight local SQLite I/O, not network calls).

use crate::domain::git_action_log::GitActionLogEntry;
use crate::infra::git_action_log_store;

#[tauri::command]
pub fn git_action_log_list(repo_root: String) -> Result<Vec<GitActionLogEntry>, String> {
    git_action_log_store::list_entries(&repo_root, 50).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_action_log_append(repo_root: String, entry: GitActionLogEntry) -> Result<(), String> {
    git_action_log_store::append_entry(&repo_root, &entry).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_action_log_mark_undone(id: String) -> Result<(), String> {
    git_action_log_store::mark_undone(&id).map_err(|e| e.to_string())
}
