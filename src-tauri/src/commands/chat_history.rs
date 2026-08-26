//! Tauri commands for persisted assistant chat history — thin wrappers over
//! `infra::chat_store`, no services layer (mirrors `commands::onboarding`,
//! which calls its own infra store directly). Plain sync `fn`s, not
//! `async`/`spawn_blocking`: these are lightweight local SQLite I/O, not
//! network calls or long-running work, matching `commands::onboarding`/
//! `commands::prefs`'s precedent for the same kind of store access.

use crate::domain::ai_tools::Task;
use crate::domain::chat::{ChatSummary, LoadedChat};
use crate::infra::chat_store;

#[tauri::command]
pub fn chat_list(repo_root: String, archived: bool) -> Result<Vec<ChatSummary>, String> {
    chat_store::list_chats(&repo_root, archived).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn chat_load_messages(chat_id: String) -> Result<LoadedChat, String> {
    chat_store::load_chat(&chat_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn chat_save(
    repo_root: String,
    chat_id: String,
    title: String,
    messages: Vec<serde_json::Value>,
    todos: Vec<Task>,
    active_plan_id: Option<String>,
    pending_resume: Option<serde_json::Value>,
) -> Result<ChatSummary, String> {
    chat_store::save_chat(
        &repo_root,
        &chat_id,
        &title,
        &messages,
        &todos,
        active_plan_id.as_deref(),
        pending_resume.as_ref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn chat_set_archived(chat_id: String, archived: bool) -> Result<(), String> {
    chat_store::set_archived(&chat_id, archived).map_err(|e| e.to_string())
}
