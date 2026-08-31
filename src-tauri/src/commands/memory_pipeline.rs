//! Fire-and-forget post-turn memory extraction. The agent loop never waits
//! on this command — `useChatHistory.saveTurn` invokes it after persisting
//! the transcript, and this returns as soon as a background `spawn_blocking`
//! is queued.

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::commands::chat_events::{chat_event_sink, NO_CHAT_TURN};
use crate::services::llm_session::LlmProviderSlot;
use crate::services::memory_pipeline::{self, MemoryExtractGuard};

#[tauri::command]
pub fn memory_extract_turn(
    app: AppHandle,
    chat_id: String,
    repo_root: String,
    llm_provider: State<'_, Arc<LlmProviderSlot>>,
    guard: State<'_, Arc<MemoryExtractGuard>>,
) -> Result<(), String> {
    let slot = llm_provider.inner().clone();
    let guard = guard.inner().clone();
    let events = chat_event_sink(&app, NO_CHAT_TURN.to_string());
    if !guard.try_start(&chat_id) {
        return Ok(());
    }
    tauri::async_runtime::spawn_blocking(move || {
        loop {
            if let Err(e) = memory_pipeline::run_pending_pass(&events, &chat_id, &repo_root, &slot) {
                eprintln!("memory extract failed for {chat_id}: {e}");
            }
            if !guard.should_rerun(&chat_id) {
                break;
            }
        }
    });
    Ok(())
}
