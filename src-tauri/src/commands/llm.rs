//! Tauri commands for the LLM provider registry: settings/credential CRUD,
//! resolving the merged provider list for the Settings picker, live model
//! listing, a lightweight "test connection" check, and streaming chat.
//! Mirrors `commands::embeddings`'s config-CRUD shape closely — most of
//! these are thin delegations to
//! `services::llm_config`/`infra::llm_credentials_store`.
//!
//! `llm_chat_stream` runs a real multi-turn tool-calling loop (`run_tool_loop`,
//! shared with `llm_chat_stream_resume`): it advertises the current
//! project's allowed tools (`services::ai_tools::llm_tool_definitions`) on
//! every round, and whenever the model requests one or more, executes them
//! via the same `services::ai_tools::execute_tool` boundary
//! `ai_execute_tool` uses — so `AiAccessMode`/a customized allowlist gates
//! what the assistant can actually do, not just what its system prompt
//! claims. Most of the loop is internal to one command call: the frontend
//! sends one message list and gets back one resolved `ChatStreamOutcome`,
//! unaware tool rounds happened at all except via the `TOOL_CALL_EVENT`/
//! `TOOL_RESULT_EVENT` pair, which the frontend renders as permanent,
//! chronological entries in the message transcript (not transient status —
//! see `src/lib/chatBlocks.ts` on the frontend side). The one case where a
//! single turn spans more than one command call: a round containing a call
//! `domain::ai_access::call_requires_confirmation` flags (per tool identity
//! for most tools, per-`op` for `memory`) resolves as
//! `ChatStreamOutcome::PendingApproval` instead, with nothing in
//! that round executed yet — except path-preflight failures
//! (`services::ai_tools::preflight_tool_call`), which return a tool error
//! immediately so an impossible write outside the documentation root never
//! shows a confirmation card. The frontend collects a user decision and calls
//! `llm_chat_stream_resume` to continue.
//!
//! Every round's request/response (or error) is optionally recorded via
//! `infra::llm_debug_log`, gated by `LlmSettings.debug_logging` — off by
//! default, toggled from the LLM settings tab. This is what makes an
//! opaque provider error (e.g. a 500 with only a trace id in its body)
//! diagnosable after the fact: the exact `ChatRequest` that produced it is
//! sitting in `~/.atlas/logs/llm.jsonl`. Covers both the tool-calling loop
//! and one-shot `llm_chat_once` / memory auto-nap callers.

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::domain::ai_access::call_requires_confirmation;
use crate::domain::ai_tools::Task;
use crate::domain::conversation_mode::ConversationMode;
use crate::domain::llm::{
    ChatEvent, ChatRequest, ChatResponse, ChatStreamOutcome, LlmMessage, LlmModelInfo,
    LlmProviderConfig, LlmRole, LlmSettings, ResolvedLlmProvider, ToolCallDecision,
};
use crate::domain::llm_rate_limit::RateLimitSnapshot;
use crate::infra::{llm_credentials_store, llm_debug_log};
use crate::services::ai_tools::{self, EmbeddingDeps};
use crate::services::embedding_state::{
    EmbeddingIndexSlot, EmbeddingProviderSlot, EmbeddingSyncGuard, IndexStoreSlot,
};
use crate::services::llm_chat::{resolve_active_file, run_tool_loop, ChatEventSink, LoopCtx};
use crate::services::llm_session::{self, ChatCancelFlag, LlmProviderSlot};
use crate::services::{
    chunk_builder::ChunkIndex, llm_config, llm_rate_limit, repo_index::RepositoryIndex,
    workspace_index::WorkspaceIndex,
};

/// Fires once per non-empty text chunk while `llm_chat_stream`'s promise is
/// still in flight. Global/unscoped, matching `SYNC_PROGRESS_EVENT`'s
/// precedent in `commands::embeddings` — this app has exactly one chat
/// panel / one in-flight conversation at a time, so no per-request id is
/// threaded through.
pub const CHAT_STREAM_DELTA_EVENT: &str = "llm:chat-stream-delta";

/// Same shape/lifecycle as `CHAT_STREAM_DELTA_EVENT`, but for a
/// reasoning-capable model's "thinking" text (`reasoning_content` on the
/// wire, see `infra::llm_providers::openai_compatible::StreamDelta`) —
/// fires while the model is still reasoning, ahead of any
/// `CHAT_STREAM_DELTA_EVENT` for that round. Never fires at all for a
/// provider/model that doesn't send `reasoning_content`.
pub const CHAT_STREAM_REASONING_EVENT: &str = "llm:chat-stream-reasoning-delta";

/// Fires immediately before executing one tool call in a `llm_chat_stream`
/// round — before, not after, so the UI can show e.g. "reading
/// docs/x.adoc…" while the (possibly slow — `SemanticSearch` can hit an
/// embedding provider) execution is actually in flight. Always followed by
/// exactly one `TOOL_RESULT_EVENT` carrying the same `id`, once execution
/// settles.
pub const TOOL_CALL_EVENT: &str = "llm:tool-call";

/// Fires once a tool call started via `TOOL_CALL_EVENT` has settled —
/// carries the same `id` so the frontend can find and close out the
/// matching entry in its transcript. Exactly one of `result`/`error` is
/// ever `Some`.
pub const TOOL_RESULT_EVENT: &str = "llm:tool-result";

/// Fires after completion tokens are recorded into the rate-limit store,
/// and after LLM settings are saved (the tracking toggle lives there) —
/// the status-bar chip refreshes without waiting for its poll interval.
pub const RATE_LIMIT_CHANGED_EVENT: &str = "llm:rate-limit-changed";

/// Turns `services::llm_chat`'s framework-free reports into real Tauri
/// events. This is the only place any of the five `llm:*` events above is
/// emitted — the chat loop itself has no `AppHandle` and no idea a UI is
/// listening.
fn chat_event_sink(app: &AppHandle) -> ChatEventSink {
    let app = app.clone();
    Arc::new(move |event: ChatEvent| {
        let _ = match event {
            ChatEvent::Delta(p) => app.emit(CHAT_STREAM_DELTA_EVENT, p),
            ChatEvent::Reasoning(p) => app.emit(CHAT_STREAM_REASONING_EVENT, p),
            ChatEvent::ToolCall(p) => app.emit(TOOL_CALL_EVENT, p),
            ChatEvent::ToolResult(p) => app.emit(TOOL_RESULT_EVENT, p),
            ChatEvent::RateLimitChanged => app.emit(RATE_LIMIT_CHANGED_EVENT, ()),
        };
    })
}


/// Requests that the currently in-flight `llm_chat_stream`/
/// `llm_chat_stream_resume` call (if any) stop as soon as it next checks —
/// mid-stream (within roughly one SSE chunk, see
/// `LlmProvider::chat_stream`'s doc comment), between tool-calling rounds,
/// or between individual tool calls within one round. Never executes a tool
/// call from the round that was in flight when this landed, which is the
/// point: a long-running or misbehaving tool-calling sequence (`WriteFile`/
/// `DeleteFile`/... included) can be stopped before its next side effect,
/// not just before its next sentence. A no-op if nothing is currently
/// running (the flag is simply left `true` until the next fresh turn resets
/// it) — safe to call speculatively, no state to check first.
#[tauri::command]
pub fn llm_cancel_chat(cancel_flag: State<'_, Arc<ChatCancelFlag>>) {
    cancel_flag.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub fn llm_get_settings() -> Result<LlmSettings, String> {
    llm_config::load_llm_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn llm_set_settings(app: AppHandle, settings: LlmSettings) -> Result<(), String> {
    llm_config::save_llm_settings(settings).map_err(|e| e.to_string())?;
    // Settings owns `rate_limit_enabled`; the chip's hook already listens
    // to this event, so a toggle takes effect without waiting for a poll.
    let _ = app.emit(RATE_LIMIT_CHANGED_EVENT, ());
    Ok(())
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
        let (provider, _resolved, _settings) =
            llm_session::resolve_provider_only(&provider_id, &llm_provider)?;
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
        let (provider, _resolved, _settings) =
            llm_session::resolve_provider_only(&provider_id, &llm_provider)?;

        let models = provider.list_models().map_err(|e| e.to_string())?;
        Ok(match models.len() {
            0 => "Соединение установлено, но провайдер не вернул ни одной модели.".to_string(),
            n => format!("Соединение установлено. Доступно моделей: {n}."),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// One non-streaming, tool-free completion — the backend surface
/// `useLlmChat`'s proactive history-compaction pass (and its reactive
/// "compact & retry" counterpart) use to ask the model to summarize an
/// older slice of the conversation. Reuses `LlmProvider::chat`, already
/// implemented by every provider but never exposed as a command until now,
/// rather than `llm_chat_stream`'s `run_tool_loop` — a summarization call
/// has no tools, needs no streaming deltas, and must never itself trigger
/// the tool-calling machinery.
#[tauri::command]
pub async fn llm_chat_once(
    app: AppHandle,
    provider_id: String,
    messages: Vec<LlmMessage>,
    llm_provider: State<'_, Arc<LlmProviderSlot>>,
) -> Result<ChatResponse, String> {
    let llm_provider = llm_provider.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<ChatResponse, String> {
        let llm_session::LlmSession { provider, model, settings, .. } =
            llm_session::resolve(&provider_id, &llm_provider)?;
        let request = ChatRequest {
            messages,
            tools: Vec::new(),
            model,
        };
        // Same debug log as the tool-calling loop — selection-AI ("Сократить")
        // and history compaction both go through here; previously only
        // `run_tool_loop` wrote to `llm.jsonl`, so one-shot failures were invisible.
        llm_debug_log::log_request(
            settings.debug_logging,
            &provider_id,
            llm_debug_log::ONCE_ROUND,
            &request,
        );
        let outcome = provider.chat(request).map_err(|e| e.to_string());
        llm_debug_log::log_chat_once_result(settings.debug_logging, &provider_id, &outcome);
        if let Ok(ref response) = outcome {
            if let Some(usage) = response.usage {
                llm_rate_limit::record(&provider_id, usage.completion_tokens);
                let _ = app.emit(RATE_LIMIT_CHANGED_EVENT, ());
            }
        }
        outcome
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Current rate-limit snapshot for the status-bar chip. Pure read — never
/// blocks a chat turn. Policy comes from the baked-in `rateLimits` preset
/// for `provider_id` (see `infra::llm_provider_manifest`) when tracking is
/// on; otherwise a hidden noop.
#[tauri::command]
pub fn llm_rate_limit_snapshot(provider_id: String) -> RateLimitSnapshot {
    llm_rate_limit::snapshot(&provider_id)
}


/// A conversation turn, streamed — a real multi-turn tool-calling loop, not
/// a single call. The frontend owns building the initial message list
/// (including any system prompt); this command resolves the provider/model
/// exactly like `llm_test_connection`, then advertises the current
/// project's allowed tools and runs `run_tool_loop`: call the model, and if
/// it requests tool calls, execute each via `services::ai_tools::
/// execute_tool` (the same boundary `ai_execute_tool` uses — so
/// `AiAccessMode`/a customized allowlist gates this exactly like it gates
/// the standalone tool endpoint), feed the results back, and call the model
/// again — until a round produces no more tool calls (the final answer), or
/// a round requests a call that needs user confirmation, in which case this
/// resolves with `ChatStreamOutcome::PendingApproval` instead and the
/// frontend must call `llm_chat_stream_resume` to continue. Text still
/// streams live as `CHAT_STREAM_DELTA_EVENT` deltas throughout every round;
/// `TOOL_CALL_EVENT` fires before each tool execution so the UI can show
/// transient status. The authoritative full text (and real token usage, if
/// the provider reported one) is returned once the loop ends — a safety net
/// against a dropped delta event, and the only place usage arrives since
/// it's a one-shot value, not a stream. `todos` is the frontend's current
/// task checklist — this backend keeps no session state of its own, so it
/// round-trips through the loop and back out via `ChatStreamOutcome`
/// exactly like `history` already does.
#[tauri::command]
pub async fn llm_chat_stream(
    app: AppHandle,
    provider_id: String,
    messages: Vec<LlmMessage>,
    todos: Vec<Task>,
    active_file_path: Option<String>,
    conversation_mode: ConversationMode,
    llm_provider: State<'_, Arc<LlmProviderSlot>>,
    cancel_flag: State<'_, Arc<ChatCancelFlag>>,
    repo_index: State<'_, Arc<RepositoryIndex>>,
    chunk_index: State<'_, Arc<ChunkIndex>>,
    embedding_index: State<'_, Arc<EmbeddingIndexSlot>>,
    index_store: State<'_, Arc<IndexStoreSlot>>,
    embedding_provider: State<'_, Arc<EmbeddingProviderSlot>>,
    sync_guard: State<'_, Arc<EmbeddingSyncGuard>>,
    workspace_index: State<'_, Arc<WorkspaceIndex>>,
) -> Result<ChatStreamOutcome, String> {
    let llm_provider = llm_provider.inner().clone();
    let cancel_flag = cancel_flag.inner().clone();
    // A *fresh* turn always starts with a clean flag — a stray cancel from
    // an already-finished previous turn must never bleed into this one.
    // `llm_chat_stream_resume` deliberately does not do this (see
    // `ChatCancelFlag`'s doc comment).
    cancel_flag.store(false, Ordering::SeqCst);
    let mut deps = EmbeddingDeps {
        repo_index: repo_index.inner().clone(),
        chunk_index: chunk_index.inner().clone(),
        embedding_index: embedding_index.inner().clone(),
        index_store: index_store.inner().clone(),
        embedding_provider: embedding_provider.inner().clone(),
        sync_guard: sync_guard.inner().clone(),
        workspace_index: workspace_index.inner().clone(),
        // Set below, once `provider`/`model` are resolved — `EditFile`'s
        // fast-apply fallback reuses the exact same provider/model this
        // turn is already using for chat, rather than resolving a second
        // one just for this.
        fast_apply: None,
        // Set below, once `scope` is resolved — the conversion needs
        // `scope.docs_root`/`scope.repo_root`. See `resolve_active_file`.
        active_file: None,
    };
    let events = chat_event_sink(&app);
    tauri::async_runtime::spawn_blocking(move || -> Result<ChatStreamOutcome, String> {
        let llm_session::LlmSession { provider, model, settings, .. } =
            llm_session::resolve(&provider_id, &llm_provider)?;
        deps.fast_apply = Some((provider.clone(), model.clone()));

        // No project open is not something the model can recover from by
        // trying again — hard-fail the whole command, same as
        // `ai_execute_tool` does for the same condition.
        let scope = ai_tools::current_scope().map_err(|e| e.to_string())?;
        deps.active_file = resolve_active_file(&scope, active_file_path);
        let tools = ai_tools::llm_tool_definitions(&scope, conversation_mode);

        let ctx = LoopCtx {
            events: &events,
            provider: provider.as_ref(),
            provider_id: &provider_id,
            model: &model,
            settings: &settings,
            deps: &deps,
            cancel_flag: &cancel_flag,
            conversation_mode,
        };
        run_tool_loop(&ctx, scope, tools, messages, 0, 0, None, todos)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Continues a conversation paused by a `ChatStreamOutcome::PendingApproval`
/// from `llm_chat_stream` (or a previous `llm_chat_stream_resume` — a
/// resumed turn can itself pause again on a later round). `history`/
/// `round`/`budget_used` must be exactly what that `PendingApproval`
/// carried, sent back unmodified — the backend keeps no server-side
/// session state between calls, so this is the entire resumable
/// checkpoint. `decisions` must cover exactly the ids of that round's
/// calls whose `requires_confirmation` was `true`; anything else is
/// rejected up front rather than silently executing calls the user never
/// actually saw. `todos` must likewise be exactly what that
/// `PendingApproval` carried, sent back unmodified.
#[tauri::command]
pub async fn llm_chat_stream_resume(
    app: AppHandle,
    provider_id: String,
    history: Vec<LlmMessage>,
    round: u32,
    budget_used: u32,
    decisions: Vec<ToolCallDecision>,
    todos: Vec<Task>,
    active_file_path: Option<String>,
    // The exact mode the paused round started with — the frontend must echo
    // back whatever it sent to the `llm_chat_stream`/prior-resume call that
    // produced this `PendingApproval`, not read a possibly-since-changed
    // live value. See `LoopCtx::conversation_mode`'s doc comment.
    conversation_mode: ConversationMode,
    llm_provider: State<'_, Arc<LlmProviderSlot>>,
    cancel_flag: State<'_, Arc<ChatCancelFlag>>,
    repo_index: State<'_, Arc<RepositoryIndex>>,
    chunk_index: State<'_, Arc<ChunkIndex>>,
    embedding_index: State<'_, Arc<EmbeddingIndexSlot>>,
    index_store: State<'_, Arc<IndexStoreSlot>>,
    embedding_provider: State<'_, Arc<EmbeddingProviderSlot>>,
    sync_guard: State<'_, Arc<EmbeddingSyncGuard>>,
    workspace_index: State<'_, Arc<WorkspaceIndex>>,
) -> Result<ChatStreamOutcome, String> {
    let llm_provider = llm_provider.inner().clone();
    // Deliberately not reset here — a cancel that landed while the
    // `PendingApproval` card this call is resuming was still on screen must
    // survive into this call so `run_tool_loop`'s first checkpoint can still
    // see it. See `ChatCancelFlag`'s doc comment.
    let cancel_flag = cancel_flag.inner().clone();
    let mut deps = EmbeddingDeps {
        repo_index: repo_index.inner().clone(),
        chunk_index: chunk_index.inner().clone(),
        embedding_index: embedding_index.inner().clone(),
        index_store: index_store.inner().clone(),
        embedding_provider: embedding_provider.inner().clone(),
        sync_guard: sync_guard.inner().clone(),
        workspace_index: workspace_index.inner().clone(),
        // Set below, once `provider`/`model` are resolved — `EditFile`'s
        // fast-apply fallback reuses the exact same provider/model this
        // turn is already using for chat, rather than resolving a second
        // one just for this.
        fast_apply: None,
        // Set below, once `scope` is resolved — see `resolve_active_file`.
        active_file: None,
    };
    let events = chat_event_sink(&app);
    tauri::async_runtime::spawn_blocking(move || -> Result<ChatStreamOutcome, String> {
        let llm_session::LlmSession { provider, model, settings, .. } =
            llm_session::resolve(&provider_id, &llm_provider)?;
        deps.fast_apply = Some((provider.clone(), model.clone()));

        let scope = ai_tools::current_scope().map_err(|e| e.to_string())?;
        deps.active_file = resolve_active_file(&scope, active_file_path);
        let tools = ai_tools::llm_tool_definitions(&scope, conversation_mode);

        let last = history
            .last()
            .ok_or_else(|| "resume: history must not be empty".to_string())?;
        if last.role != LlmRole::Assistant || last.tool_calls.is_empty() {
            return Err(
                "resume: history must end with the assistant's tool-call turn".to_string()
            );
        }
        let calls = last.tool_calls.clone();

        let expected: HashSet<&str> = calls
            .iter()
            .filter(|c| call_requires_confirmation(&c.name, &c.arguments))
            .map(|c| c.id.as_str())
            .collect();
        let provided: HashSet<&str> = decisions.iter().map(|d| d.id.as_str()).collect();
        if expected != provided {
            return Err(
                "resume: decisions do not match this round's pending calls".to_string()
            );
        }

        let ctx = LoopCtx {
            events: &events,
            provider: provider.as_ref(),
            provider_id: &provider_id,
            model: &model,
            settings: &settings,
            deps: &deps,
            cancel_flag: &cancel_flag,
            conversation_mode,
        };
        run_tool_loop(&ctx, scope, tools, history, round, budget_used, Some((calls, decisions)), todos)
    })
    .await
    .map_err(|e| e.to_string())?
}
