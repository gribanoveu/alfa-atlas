//! Tauri commands for the LLM provider registry: settings/credential CRUD,
//! resolving the merged provider list for the Settings picker, live model
//! listing, a lightweight "test connection" check, and streaming chat.
//! Mirrors `commands::embeddings`'s config-CRUD shape closely — most of
//! these are thin delegations to
//! `services::llm_config`/`infra::llm_credentials_store`.
//!
//! `llm_chat_stream` now runs a real multi-turn tool-calling loop: it
//! advertises the current project's allowed `ReadFile`/`ListFiles`/
//! `SemanticSearch` tools (`services::ai_tools::llm_tool_definitions`) on
//! every round, and whenever the model requests one or more, executes them
//! via the same `services::ai_tools::execute_tool` boundary
//! `ai_execute_tool` uses — so `AiAccessMode`/a customized allowlist now
//! gates what the assistant can actually read, not just what its system
//! prompt claims. The loop is entirely internal to this command: the
//! frontend still sends one plain message list and gets back one resolved
//! `ChatStreamResult`, unaware tool rounds happened at all except via the
//! `TOOL_CALL_EVENT` status event.
//!
//! Every round's request/response (or error) is optionally recorded via
//! `infra::llm_debug_log`, gated by `LlmSettings.debug_logging` — off by
//! default, toggled from the LLM settings tab. This is what makes an
//! opaque provider error (e.g. a 500 with only a trace id in its body)
//! diagnosable after the fact: the exact `ChatRequest` that produced it is
//! sitting in `~/.atlas/logs/llm.jsonl`.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, State};

use crate::commands::embeddings::{
    EmbeddingIndexSlot, EmbeddingProviderSlot, EmbeddingSyncGuard, IndexStoreSlot,
};
use crate::domain::llm::{
    sanitize_tool_call_arguments, ChatRequest, ChatStreamResult, LlmMessage, LlmModelInfo,
    LlmProvider, LlmProviderConfig, LlmRole, LlmSettings, ResolvedLlmProvider,
};
use crate::infra::{llm_credentials_store, llm_debug_log, llm_providers};
use crate::services::ai_tools::{self, EmbeddingDeps};
use crate::services::chunk_builder::ChunkIndex;
use crate::services::llm_config;
use crate::services::repo_index::RepositoryIndex;

/// Fires once per non-empty text chunk while `llm_chat_stream`'s promise is
/// still in flight. Global/unscoped, matching `SYNC_PROGRESS_EVENT`'s
/// precedent in `commands::embeddings` — this app has exactly one chat
/// panel / one in-flight conversation at a time, so no per-request id is
/// threaded through.
pub const CHAT_STREAM_DELTA_EVENT: &str = "llm:chat-stream-delta";

/// Fires immediately before executing one tool call in a `llm_chat_stream`
/// round — before, not after, so the UI can show e.g. "reading
/// docs/x.adoc…" while the (possibly slow — `SemanticSearch` can hit an
/// embedding provider) execution is actually in flight. No matching "done"
/// event exists: the frontend clears its status either when a real text
/// delta resumes streaming or when the turn ends.
pub const TOOL_CALL_EVENT: &str = "llm:tool-call";

/// A misbehaving/looping model shouldn't be able to hold the UI in a
/// "thinking" state indefinitely — this caps how many model↔tool round
/// trips one `llm_chat_stream` call will run before hard-failing.
const MAX_TOOL_ITERATIONS: usize = 6;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatStreamDeltaPayload {
    delta: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolCallEventPayload {
    name: String,
    /// Raw JSON-encoded string, same as `LlmToolCall::arguments` — the
    /// frontend parses it if it wants structured display, this event
    /// doesn't pre-parse it.
    arguments: String,
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

/// A conversation turn, streamed — now a real multi-turn tool-calling loop,
/// not a single call. The frontend owns building the initial message list
/// (including any system prompt); this command resolves the provider/model
/// exactly like `llm_test_connection`, then advertises the current
/// project's allowed tools and loops: call the model, and if it requests
/// tool calls, execute each via `services::ai_tools::execute_tool` (the
/// same boundary `ai_execute_tool` uses — so `AiAccessMode`/a customized
/// allowlist gates this exactly like it gates the standalone tool
/// endpoint), feed the results back, and call the model again — until a
/// round produces no more tool calls, which is the final answer returned
/// here. Text still streams live as `CHAT_STREAM_DELTA_EVENT` deltas
/// throughout every round; `TOOL_CALL_EVENT` fires before each tool
/// execution so the UI can show transient status. The authoritative full
/// text (and real token usage, if the provider reported one) is returned
/// once the loop ends — a safety net against a dropped delta event, and the
/// only place usage arrives since it's a one-shot value, not a stream.
#[tauri::command]
pub async fn llm_chat_stream(
    app: AppHandle,
    provider_id: String,
    messages: Vec<LlmMessage>,
    llm_provider: State<'_, Arc<LlmProviderSlot>>,
    repo_index: State<'_, Arc<RepositoryIndex>>,
    chunk_index: State<'_, Arc<ChunkIndex>>,
    embedding_index: State<'_, Arc<EmbeddingIndexSlot>>,
    index_store: State<'_, Arc<IndexStoreSlot>>,
    embedding_provider: State<'_, Arc<EmbeddingProviderSlot>>,
    sync_guard: State<'_, Arc<EmbeddingSyncGuard>>,
) -> Result<ChatStreamResult, String> {
    let llm_provider = llm_provider.inner().clone();
    let deps = EmbeddingDeps {
        repo_index: repo_index.inner().clone(),
        chunk_index: chunk_index.inner().clone(),
        embedding_index: embedding_index.inner().clone(),
        index_store: index_store.inner().clone(),
        embedding_provider: embedding_provider.inner().clone(),
        sync_guard: sync_guard.inner().clone(),
    };
    tauri::async_runtime::spawn_blocking(move || -> Result<ChatStreamResult, String> {
        let settings = llm_config::load_llm_settings().map_err(|e| e.to_string())?;
        let resolved =
            llm_config::resolve_provider(&provider_id, &settings).map_err(|e| e.to_string())?;
        let api_key = llm_credentials_store::get_api_key(&provider_id);
        let provider = ensure_llm_provider(&llm_provider, &resolved, api_key)?;
        let model = llm_config::effective_model(&resolved, provider.as_ref())
            .map_err(|e| e.to_string())?;

        // No project open is not something the model can recover from by
        // trying again — hard-fail the whole command, same as
        // `ai_execute_tool` does for the same condition.
        let scope = ai_tools::current_scope().map_err(|e| e.to_string())?;
        let tools = ai_tools::llm_tool_definitions(&scope);

        let on_delta = |delta: &str| {
            let _ = app.emit(CHAT_STREAM_DELTA_EVENT, ChatStreamDeltaPayload { delta: delta.to_string() });
        };

        let mut history = messages;
        let mut round: u32 = 0;
        for _ in 0..MAX_TOOL_ITERATIONS {
            round += 1;
            let request = ChatRequest { messages: history.clone(), tools: tools.clone(), model: model.clone() };
            llm_debug_log::log_request(settings.debug_logging, &provider_id, round, &request);
            let raw_result = provider.chat_stream(request, &on_delta);
            llm_debug_log::log_response(settings.debug_logging, &provider_id, round, &raw_result);
            let result = raw_result.map_err(|e| e.to_string())?;

            if result.tool_calls.is_empty() {
                return Ok(result);
            }

            // Round-trip the assistant's tool-call turn back into history
            // so the next request shows the provider its own prior
            // request. `None` content for a tool-only turn matches the
            // wire reality (`LlmMessage::content`'s own doc comment).
            // `sanitize_tool_call_arguments` — not the raw `result.
            // tool_calls` — is what gets echoed: a model occasionally
            // streams malformed `arguments` JSON, and at least one
            // real-world gateway 500s server-side when it's later echoed
            // back verbatim (the loop below still uses the *unsanitized*
            // `result.tool_calls` for `parse_tool_call`, so the model still
            // gets an honest error about what it actually sent).
            history.push(LlmMessage {
                role: LlmRole::Assistant,
                content: if result.text.is_empty() { None } else { Some(result.text.clone()) },
                tool_call_id: None,
                tool_calls: sanitize_tool_call_arguments(&result.tool_calls),
            });

            for call in &result.tool_calls {
                let _ = app.emit(
                    TOOL_CALL_EVENT,
                    ToolCallEventPayload { name: call.name.clone(), arguments: call.arguments.clone() },
                );
                // A bad tool call (unknown name, malformed arguments, a
                // NotAllowed hit against the allowlist, a missing file,
                // ...) is always recoverable-by-the-model, never a hard
                // failure of the whole turn — the model discovering an
                // access-mode boundary mid-conversation is legitimate,
                // expected behavior, not a bug.
                let content = match ai_tools::parse_tool_call(call)
                    .and_then(|parsed| ai_tools::execute_tool(&scope, parsed, &deps))
                {
                    Ok(tool_result) => serde_json::to_string(&tool_result)
                        .unwrap_or_else(|_| "Error: failed to serialize tool result".to_string()),
                    Err(e) => format!("Error: {e}"),
                };
                history.push(LlmMessage {
                    role: LlmRole::Tool,
                    content: Some(content),
                    tool_call_id: Some(call.id.clone()),
                    tool_calls: vec![],
                });
            }
        }
        Err(format!(
            "assistant did not produce a final answer within {MAX_TOOL_ITERATIONS} tool-call rounds"
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}
