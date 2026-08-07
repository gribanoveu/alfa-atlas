//! Tauri commands for the LLM provider registry: settings/credential CRUD,
//! resolving the merged provider list for the Settings picker, live model
//! listing, a lightweight "test connection" check, and streaming chat.
//! Mirrors `commands::embeddings`'s config-CRUD shape closely — most of
//! these are thin delegations to
//! `services::llm_config`/`infra::llm_credentials_store`.
//!
//! No tool-execution loop calls into this yet — `llm_chat_stream` is a
//! plain conversation turn (no `tools`), the first real caller of
//! `LlmProvider::chat_stream`.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, State};

use crate::domain::llm::{
    ChatRequest, LlmMessage, LlmModelInfo, LlmProvider, LlmProviderConfig, LlmSettings,
    ResolvedLlmProvider,
};
use crate::infra::{llm_credentials_store, llm_providers};
use crate::services::llm_config;

/// Fires once per non-empty text chunk while `llm_chat_stream`'s promise is
/// still in flight. Global/unscoped, matching `SYNC_PROGRESS_EVENT`'s
/// precedent in `commands::embeddings` — this app has exactly one chat
/// panel / one in-flight conversation at a time, so no per-request id is
/// threaded through.
pub const CHAT_STREAM_DELTA_EVENT: &str = "llm:chat-stream-delta";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatStreamDeltaPayload {
    delta: String,
}

/// Caches the constructed `LlmProvider` across calls, same reasoning as
/// `commands::embeddings::EmbeddingProviderSlot`: keyed by
/// `(resolved, api_key)` rather than just the provider id, so a key
/// rotation or a settings-layer override change (a different `base_url`/
/// `trusted_cert_pem`) invalidates the cache instead of silently reusing a
/// stale `ureq::Agent`.
pub type LlmProviderSlot = Mutex<Option<(ResolvedLlmProvider, Option<String>, Arc<dyn LlmProvider>)>>;

pub(crate) fn ensure_llm_provider(
    slot: &LlmProviderSlot,
    resolved: &ResolvedLlmProvider,
    api_key: Option<String>,
) -> Result<Arc<dyn LlmProvider>, String> {
    let mut guard = slot.lock().map_err(|_| "llm provider lock poisoned".to_string())?;
    let stale = !matches!(guard.as_ref(), Some((r, k, _)) if r == resolved && *k == api_key);
    if stale {
        let provider =
            llm_providers::provider_for(resolved, api_key.clone()).map_err(|e| e.to_string())?;
        *guard = Some((resolved.clone(), api_key, Arc::from(provider)));
    }
    Ok(guard.as_ref().expect("just set above if missing").2.clone())
}

#[tauri::command]
pub fn llm_get_settings() -> Result<LlmSettings, String> {
    llm_config::load_llm_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn llm_set_settings(settings: LlmSettings) -> Result<(), String> {
    llm_config::save_llm_settings(settings).map_err(|e| e.to_string())
}

/// Every provider available for the Settings picker — every compiled-in
/// system preset (merged with its override, if any) plus every custom
/// provider, in `services::llm_config::list_resolved_providers`'s order.
#[tauri::command]
pub fn llm_list_providers() -> Result<Vec<ResolvedLlmProvider>, String> {
    let settings = llm_config::load_llm_settings().map_err(|e| e.to_string())?;
    Ok(llm_config::list_resolved_providers(&settings))
}

#[tauri::command]
pub fn llm_upsert_provider(config: LlmProviderConfig) -> Result<(), String> {
    let mut settings = llm_config::load_llm_settings().map_err(|e| e.to_string())?;
    llm_config::upsert_provider_config(&mut settings, config);
    llm_config::save_llm_settings(settings).map_err(|e| e.to_string())
}

/// For a system provider id, this only clears its settings-layer override
/// (label/base_url/model/cert revert to the compiled-in manifest values) —
/// it can never remove the manifest preset itself. True removal of a
/// system provider is a manifest-edit-and-rebuild operation (see
/// `infra::llm_provider_manifest`'s doc comment); the Settings UI should
/// not offer this command for system-provider rows at all.
#[tauri::command]
pub fn llm_remove_provider(provider_id: String) -> Result<(), String> {
    let mut settings = llm_config::load_llm_settings().map_err(|e| e.to_string())?;
    llm_config::remove_provider_config(&mut settings, &provider_id);
    llm_config::save_llm_settings(settings).map_err(|e| e.to_string())?;
    llm_credentials_store::delete_api_key(&provider_id)
}

/// Write-only, mirrors `commands::embeddings::embedding_set_remote_api_key`:
/// the key itself is never returned from a command, only whether one is set.
#[tauri::command]
pub fn llm_set_api_key(provider_id: String, api_key: String) -> Result<(), String> {
    llm_credentials_store::save_api_key(&provider_id, &api_key)
}

#[tauri::command]
pub fn llm_has_api_key(provider_id: String) -> bool {
    llm_credentials_store::has_api_key(&provider_id)
}

#[tauri::command]
pub async fn llm_list_models(
    provider_id: String,
    llm_provider: State<'_, Arc<LlmProviderSlot>>,
) -> Result<Vec<LlmModelInfo>, String> {
    let llm_provider = llm_provider.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<LlmModelInfo>, String> {
        let settings = llm_config::load_llm_settings().map_err(|e| e.to_string())?;
        let resolved =
            llm_config::resolve_provider(&provider_id, &settings).map_err(|e| e.to_string())?;
        let api_key = llm_credentials_store::get_api_key(&provider_id);
        let provider = ensure_llm_provider(&llm_provider, &resolved, api_key)?;
        provider.list_models().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// End-to-end verification surface for the whole client (config →
/// credentials → TLS → HTTP → response parsing), without needing a chat UI
/// or the tool-execution loop to exist — a "Проверить соединение" button
/// in Settings. Only calls `list_models`, not `chat`: fetching the model
/// list already exercises the full stack (auth header, TLS trust, JSON
/// parsing) without spending a completion or requiring a resolvable model.
#[tauri::command]
pub async fn llm_test_connection(
    provider_id: String,
    llm_provider: State<'_, Arc<LlmProviderSlot>>,
) -> Result<String, String> {
    let llm_provider = llm_provider.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let settings = llm_config::load_llm_settings().map_err(|e| e.to_string())?;
        let resolved =
            llm_config::resolve_provider(&provider_id, &settings).map_err(|e| e.to_string())?;
        let api_key = llm_credentials_store::get_api_key(&provider_id);
        let provider = ensure_llm_provider(&llm_provider, &resolved, api_key)?;

        let models = provider.list_models().map_err(|e| e.to_string())?;
        Ok(match models.len() {
            0 => "Соединение установлено, но провайдер не вернул ни одной модели.".to_string(),
            n => format!("Соединение установлено. Доступно моделей: {n}."),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A plain conversation turn, streamed. The frontend owns building the full
/// message list (including any system prompt) — this command does no
/// message-list logic of its own. Resolves the provider/model exactly like
/// `llm_test_connection`, then streams the reply as
/// `CHAT_STREAM_DELTA_EVENT` deltas while also returning the authoritative
/// full text once the stream ends (a safety net for the frontend against a
/// dropped event).
#[tauri::command]
pub async fn llm_chat_stream(
    app: AppHandle,
    provider_id: String,
    messages: Vec<LlmMessage>,
    llm_provider: State<'_, Arc<LlmProviderSlot>>,
) -> Result<String, String> {
    let llm_provider = llm_provider.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let settings = llm_config::load_llm_settings().map_err(|e| e.to_string())?;
        let resolved =
            llm_config::resolve_provider(&provider_id, &settings).map_err(|e| e.to_string())?;
        let api_key = llm_credentials_store::get_api_key(&provider_id);
        let provider = ensure_llm_provider(&llm_provider, &resolved, api_key)?;
        let model = llm_config::effective_model(&resolved, provider.as_ref())
            .map_err(|e| e.to_string())?;

        let request = ChatRequest { messages, tools: vec![], model };
        let on_delta = |delta: &str| {
            let _ = app.emit(CHAT_STREAM_DELTA_EVENT, ChatStreamDeltaPayload { delta: delta.to_string() });
        };
        provider.chat_stream(request, &on_delta).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
