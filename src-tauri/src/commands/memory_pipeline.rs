//! Fire-and-forget post-turn memory extraction. The agent loop never waits
//! on this command — `useChatHistory.saveTurn` invokes it after persisting
//! the transcript, and this returns as soon as a background `spawn_blocking`
//! is queued.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, State};

use crate::commands::llm::RATE_LIMIT_CHANGED_EVENT;
use crate::services::llm_session::{self, LlmProviderSlot};
use crate::domain::llm::{ChatRequest, LlmMessage, LlmRole};
use crate::domain::memory_extract::pending_turn;
use crate::domain::memory_policy::MemoryPolicyConfig;
use crate::infra::{chat_store, llm_debug_log};
use crate::services::{llm_config, llm_rate_limit, memory_pipeline};

/// Per-chat in-flight + dirty flag so a save that lands while a pass is
/// running is not dropped: the running job loops until the dirty bit is
/// clear.
pub struct MemoryExtractGuard {
    inner: Mutex<GuardInner>,
}

struct GuardInner {
    in_flight: HashSet<String>,
    dirty: HashSet<String>,
}

impl MemoryExtractGuard {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(GuardInner {
                in_flight: HashSet::new(),
                dirty: HashSet::new(),
            }),
        }
    }

    /// `true` = this caller should run the job. `false` = already running;
    /// the current pass will re-check after it finishes.
    fn try_start(&self, chat_id: &str) -> bool {
        let Ok(mut g) = self.inner.lock() else {
            return false;
        };
        if g.in_flight.contains(chat_id) {
            g.dirty.insert(chat_id.to_string());
            false
        } else {
            g.in_flight.insert(chat_id.to_string());
            g.dirty.remove(chat_id);
            true
        }
    }

    /// After one pass: `true` if another pass is needed.
    fn should_rerun(&self, chat_id: &str) -> bool {
        let Ok(mut g) = self.inner.lock() else {
            return false;
        };
        if g.dirty.remove(chat_id) {
            true
        } else {
            g.in_flight.remove(chat_id);
            false
        }
    }
}

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
    if !guard.try_start(&chat_id) {
        return Ok(());
    }
    tauri::async_runtime::spawn_blocking(move || {
        loop {
            if let Err(e) = run_one_pass(&app, &chat_id, &repo_root, &slot) {
                eprintln!("memory extract failed for {chat_id}: {e}");
            }
            if !guard.should_rerun(&chat_id) {
                break;
            }
        }
    });
    Ok(())
}

fn run_one_pass(
    app: &AppHandle,
    chat_id: &str,
    repo_root: &str,
    slot: &LlmProviderSlot,
) -> Result<(), String> {
    let settings = llm_config::load_llm_settings().map_err(|e| e.to_string())?;
    if !settings.memory_extraction_enabled {
        return Ok(());
    }
    let stored_root = chat_store::chat_repo_root(chat_id).map_err(|e| e.to_string())?;
    if stored_root != repo_root {
        return Err(format!(
            "chat {chat_id} belongs to {stored_root}, not {repo_root}"
        ));
    }

    let watermark = chat_store::memory_extracted_ordinal(chat_id).map_err(|e| e.to_string())?;
    let loaded = chat_store::load_chat(chat_id).map_err(|e| e.to_string())?;
    let Some(pending) = pending_turn(&loaded.messages, watermark) else {
        return Ok(());
    };

    if let Some(transcript) = pending.transcript {
        let Some(provider_id) = llm_config::effective_active_provider_id(&settings) else {
            // No resolvable provider — leave the watermark so a later save retries.
            return Ok(());
        };

        let llm_session::LlmSession { provider, model, .. } =
            llm_session::resolve(&provider_id, slot)?;
        let debug = settings.debug_logging;
        let app_handle = app.clone();
        let provider_id_for_log = provider_id.clone();

        let mut llm = |prompt: &str| -> Result<String, String> {
            let request = ChatRequest {
                messages: vec![LlmMessage {
                    role: LlmRole::User,
                    content: Some(prompt.to_string()),
                    tool_call_id: None,
                    tool_calls: vec![],
                }],
                tools: Vec::new(),
                model: model.clone(),
            };
            llm_debug_log::log_request(debug, &provider_id_for_log, llm_debug_log::ONCE_ROUND, &request);
            let outcome = provider.chat(request).map_err(|e| e.to_string());
            llm_debug_log::log_chat_once_result(debug, &provider_id_for_log, &outcome);
            if let Ok(ref response) = outcome {
                if let Some(usage) = response.usage {
                    llm_rate_limit::record(&provider_id_for_log, usage.completion_tokens);
                    let _ = app_handle.emit(RATE_LIMIT_CHANGED_EVENT, ());
                }
            }
            outcome.map(|resp| resp.content.unwrap_or_default())
        };

        let config = MemoryPolicyConfig::from_threshold(settings.memory_confidence_threshold);
        let root = PathBuf::from(repo_root);
        // LLM failure must not advance the watermark — the next save retries.
        memory_pipeline::run_turn(&transcript, &root, &config, &mut llm)
            .map_err(|e| e.to_string())?;
    }

    chat_store::set_memory_extracted_ordinal(chat_id, pending.last_ordinal)
        .map_err(|e| e.to_string())?;
    Ok(())
}
