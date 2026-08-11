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
//! whose `domain::ai_access::ToolName::requires_confirmation` is `true`
//! resolves as `ChatStreamOutcome::PendingApproval` instead, with nothing in
//! that round executed — the frontend collects a user decision and calls
//! `llm_chat_stream_resume` to continue.
//!
//! Every round's request/response (or error) is optionally recorded via
//! `infra::llm_debug_log`, gated by `LlmSettings.debug_logging` — off by
//! default, toggled from the LLM settings tab. This is what makes an
//! opaque provider error (e.g. a 500 with only a trace id in its body)
//! diagnosable after the fact: the exact `ChatRequest` that produced it is
//! sitting in `~/.atlas/logs/llm.jsonl`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, State};

use crate::commands::embeddings::{
    EmbeddingIndexSlot, EmbeddingProviderSlot, EmbeddingSyncGuard, IndexStoreSlot,
};
use crate::domain::ai_access::ToolName;
use crate::domain::ai_tools::{Task, ToolResult, ToolScope};
use crate::domain::llm::{
    sanitize_tool_call_arguments, ChatDone, ChatRequest, ChatStreamOutcome, ChatStreamResult,
    LlmMessage, LlmModelInfo, LlmProvider, LlmProviderConfig, LlmRole, LlmSettings, LlmToolCall,
    LlmToolDefinition, PendingApproval, PendingToolCall, ResolvedLlmProvider, ToolCallDecision,
};
use crate::domain::paths;
use crate::domain::repo_index::FileId;
use crate::infra::{llm_credentials_store, llm_debug_log, llm_providers};
use crate::services::ai_tools::{self, EmbeddingDeps, ToolCallLogContext};
use crate::services::chunk_builder::ChunkIndex;
use crate::services::llm_config;
use crate::services::repo_index::RepositoryIndex;
use crate::services::workspace_index::WorkspaceIndex;

/// Fires once per non-empty text chunk while `llm_chat_stream`'s promise is
/// still in flight. Global/unscoped, matching `SYNC_PROGRESS_EVENT`'s
/// precedent in `commands::embeddings` — this app has exactly one chat
/// panel / one in-flight conversation at a time, so no per-request id is
/// threaded through.
pub const CHAT_STREAM_DELTA_EVENT: &str = "llm:chat-stream-delta";

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

/// A misbehaving/looping model shouldn't be able to hold the UI in a
/// "thinking" state indefinitely — this caps how many model↔tool round
/// trips one `llm_chat_stream` call will run before hard-failing. Kept as
/// a backstop alongside `MAX_TOOL_BUDGET` (a misconfigured/zero tool
/// weight must never make the loop unstoppable), but `MAX_TOOL_BUDGET` is
/// the more sensitive limit in practice — see its doc comment.
const MAX_TOOL_ITERATIONS: usize = 50;

/// Converts the frontend's docs-root-relative `EditorTab.path` (sent
/// verbatim, same convention `embedding_set_priority_files` already
/// establishes) into `FileId` space (`repo_root`-relative) for
/// `EmbeddingDeps::active_file`. `None` on any resolution failure (no path,
/// or a path outside `scope.repo_root`) — degrades to "no boost" rather
/// than failing the whole chat turn over a best-effort ranking hint.
fn resolve_active_file(scope: &ToolScope, active_file_path: Option<String>) -> Option<FileId> {
    let path = active_file_path?;
    let absolute = paths::join_relative(&scope.docs_root, &path).ok()?;
    paths::relative_to_lenient(&scope.repo_root, &absolute).ok().map(FileId)
}

/// The primary loop limit: unlike `MAX_TOOL_ITERATIONS`, this weighs each
/// round by what it actually cost (`round_cost`, sum of
/// `ToolName::loop_weight` over that round's calls) rather than counting
/// every round as "1" regardless of whether it called the cheap
/// `ListFiles`/`ReadFile` or the much more expensive `SemanticSearch`.
/// Sized so an all-cheap-tool sequence is still effectively bounded by
/// `MAX_TOOL_ITERATIONS` (no regression there), while a
/// `SemanticSearch`-heavy sequence now cuts off around 10 calls instead of
/// 20.
const MAX_TOOL_BUDGET: u32 = 250;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatStreamDeltaPayload {
    delta: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolCallEventPayload {
    /// The model's own `LlmToolCall::id` — lets the frontend correlate this
    /// call with its later `ToolResultEventPayload` regardless of how many
    /// other calls/rounds happen in between.
    id: String,
    name: String,
    /// Raw JSON-encoded string, same as `LlmToolCall::arguments` — the
    /// frontend parses it if it wants structured display, this event
    /// doesn't pre-parse it.
    arguments: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolResultEventPayload {
    /// Matches the `id` on the `ToolCallEventPayload` this settles.
    id: String,
    /// `Some` on success — the same typed `ToolResult` that gets
    /// JSON-serialized into the wire `content` sent back to the model, just
    /// cloned rather than reserialized into a display string here.
    /// Formatting it into a human-readable summary is the frontend's job
    /// (`describeToolResult` in `src/lib/assistantConfig.ts`), matching how
    /// `describeToolActivity` already handles the "what is being done" line
    /// client-side rather than this command inventing display text.
    result: Option<ToolResult>,
    /// `Some` on failure — `ToolError`'s `Display` text (the same string
    /// that already goes into the `Tool` message's `content` as
    /// `"Error: {e}"`).
    error: Option<String>,
}

/// Caches the constructed `LlmProvider` across calls, same reasoning as
/// `commands::embeddings::EmbeddingProviderSlot`: keyed by
/// `(resolved, api_key)` rather than just the provider id, so a key
/// rotation or a settings-layer override change (a different `base_url`/
/// `trusted_cert_pem`) invalidates the cache instead of silently reusing a
/// stale `ureq::Agent`.
pub type LlmProviderSlot = Mutex<Option<(ResolvedLlmProvider, Option<String>, Arc<dyn LlmProvider>)>>;

/// One flag for "the user asked the in-flight turn to stop" — this app has
/// exactly one chat panel / one in-flight conversation at a time (same
/// assumption `CHAT_STREAM_DELTA_EVENT` already makes), so a single
/// `Arc<AtomicBool>` needs no per-turn/per-request id to disambiguate.
/// `llm_chat_stream` resets this to `false` at the start of every *fresh*
/// turn (never `llm_chat_stream_resume`, which continues a turn already in
/// progress and must not lose a cancellation that landed while a
/// `PendingApproval` card was showing — see `llm_cancel_chat`'s doc
/// comment); `run_tool_loop` polls it at the checkpoints documented on its
/// own doc comment and resolves `ChatStreamOutcome::Cancelled` instead of
/// continuing once it reads `true`.
pub type ChatCancelFlag = AtomicBool;

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

/// One round's cost against `MAX_TOOL_BUDGET` — the sum of
/// `ToolName::loop_weight` over every call the round contains (a round can
/// bundle several parallel calls, each adding to the cost). An
/// unrecognized/malformed tool name (fails `ToolName::from_wire_name`)
/// costs `1`, the same floor as the cheapest real tool, so budget always
/// makes forward progress even for a hallucinated tool name. A pure
/// function so it's testable without a mock `LlmProvider`.
fn round_cost(calls: &[LlmToolCall]) -> u32 {
    calls
        .iter()
        .map(|c| ToolName::from_wire_name(&c.name).map(ToolName::loop_weight).unwrap_or(1))
        .sum()
}

/// Bundles the read-only pieces `run_tool_loop` needs so its own signature
/// doesn't grow a long, drifting parameter list — everything here is fixed
/// for the lifetime of one `llm_chat_stream`/`llm_chat_stream_resume` call.
struct LoopCtx<'a> {
    app: &'a AppHandle,
    provider: &'a dyn LlmProvider,
    provider_id: &'a str,
    model: &'a str,
    settings: &'a LlmSettings,
    deps: &'a EmbeddingDeps,
    cancel_flag: &'a ChatCancelFlag,
}

/// The shared tool-calling loop both `llm_chat_stream` (fresh start,
/// `resume: None`) and `llm_chat_stream_resume` (continuing a paused round,
/// `resume: Some((calls, decisions))`) run. `scope`/`tools` are `mut`
/// because a successful `RequestFullRepoAccess` widens them mid-loop — the
/// escalation must take effect within the same turn, not just the next one,
/// or the assistant would report success while its very next tool call
/// stays walled off at the old boundary.
///
/// Pauses (returns `ChatStreamOutcome::PendingApproval`) the instant a
/// *fresh* round (never a resumed one — a resumed round's decisions are
/// already known) contains any call whose `ToolName::requires_confirmation`
/// is `true`. Nothing in that round executes — not even other, non-risky
/// calls bundled into the same round — so there's no partial-round state to
/// track across the stateless hop back to the frontend.
///
/// Also resolves early, with `ChatStreamOutcome::Cancelled` instead of
/// `Done`, if `ctx.cancel_flag` (set by `llm_cancel_chat`) reads `true` at
/// either of two checkpoints:
/// - The top of the outer `loop`, before deciding whether to call the model
///   or process a resumed round's already-known calls — catches "stop
///   between rounds," "stop between individual tool calls within one
///   round" (the `for call in &tool_calls` loop below `break`s as soon as it
///   sees the flag, falling through to this same checkpoint on the next
///   iteration rather than duplicating the check), and "stop while a
///   `PendingApproval` card was showing" (the frontend both sets the flag
///   and auto-denies every pending call before calling
///   `llm_chat_stream_resume`, so this checkpoint fires before any of those
///   calls — now-moot, since the model is never asked to react to them —
///   actually execute).
/// - Immediately after `ctx.provider.chat_stream` returns for a fresh
///   round, before either the "no tool calls" (`Done`) branch or the
///   pending-approval check — covers a stop that landed mid-stream (`text`
///   is whatever had accumulated in *this* round before it broke early) or
///   right as a round finished. Never executes any of that round's tool
///   calls, confirmation-gated or not — this is what lets a stop actually
///   pre-empt a `WriteFile`/`DeleteFile`/... about to run, not just the
///   model's next sentence.
///
/// At the first checkpoint, `result.text` is always `""` — by construction
/// the frontend's trailing transcript block is a settled tool-call block at
/// that point (a round's own streamed prose, if any, is always closed off
/// by whatever tool-call block followed it — see `chatBlocks.ts`'s
/// `appendDeltaToBlocks` doc comment), so an empty string correctly leaves
/// it untouched via `correctTrailingText` on the frontend rather than
/// clobbering it.
fn run_tool_loop(
    ctx: &LoopCtx,
    mut scope: ToolScope,
    mut tools: Vec<LlmToolDefinition>,
    mut history: Vec<LlmMessage>,
    mut round: u32,
    mut budget_used: u32,
    mut resume: Option<(Vec<LlmToolCall>, Vec<ToolCallDecision>)>,
    mut todos: Vec<Task>,
) -> Result<ChatStreamOutcome, String> {
    loop {
        // Checkpoint 1 — see this function's doc comment for exactly which
        // "stop" scenarios this catches. Placed before the iteration-limit
        // check too: a cancelled turn should report as cancelled, not as
        // having hit `MAX_TOOL_ITERATIONS`, if both would otherwise fire on
        // the same iteration.
        if ctx.cancel_flag.load(Ordering::SeqCst) {
            return Ok(ChatStreamOutcome::Cancelled(ChatDone {
                result: ChatStreamResult { text: String::new(), usage: None, tool_calls: vec![] },
                todos,
            }));
        }
        if round >= MAX_TOOL_ITERATIONS as u32 || budget_used >= MAX_TOOL_BUDGET {
            return Err(format!(
                "Ассистент не дал окончательный ответ за {MAX_TOOL_ITERATIONS} раундов обращения к инструментам (бюджет {budget_used}/{MAX_TOOL_BUDGET})"
            ));
        }
        round += 1;

        let (tool_calls, decisions): (Vec<LlmToolCall>, Vec<ToolCallDecision>) =
            if let Some((calls, decisions)) = resume.take() {
                // Resuming: this round's calls and the caller's decisions on
                // them are already known — skip calling the model, skip
                // re-pushing the assistant turn (it's already the tail of
                // `history`, since `PendingApproval.history` included it).
                // Charged here (not just on the fresh pass that first
                // computed these calls, below) so a paused-then-resumed
                // round is billed on both passes, same as `round` already
                // double-counts it — otherwise pausing would be a free way
                // to dodge the budget.
                budget_used += round_cost(&calls);
                (calls, decisions)
            } else {
                let request = ChatRequest {
                    messages: history.clone(),
                    tools: tools.clone(),
                    model: ctx.model.to_string(),
                };
                llm_debug_log::log_request(ctx.settings.debug_logging, ctx.provider_id, round, &request);
                let on_delta = |delta: &str| {
                    let _ = ctx.app.emit(
                        CHAT_STREAM_DELTA_EVENT,
                        ChatStreamDeltaPayload { delta: delta.to_string() },
                    );
                };
                let cancelled = || ctx.cancel_flag.load(Ordering::SeqCst);
                let raw_result = ctx.provider.chat_stream(request, &on_delta, &cancelled);
                llm_debug_log::log_response(ctx.settings.debug_logging, ctx.provider_id, round, &raw_result);
                let result = raw_result.map_err(|e| e.to_string())?;

                // Checkpoint 2 — see this function's doc comment. Checked
                // before either branch below so a stop that landed exactly
                // as this round finished (mid-stream, or naturally) never
                // reaches the pending-approval check or executes any of
                // this round's tool calls, confirmation-gated or not.
                if cancelled() {
                    return Ok(ChatStreamOutcome::Cancelled(ChatDone { result, todos }));
                }

                if result.tool_calls.is_empty() {
                    return Ok(ChatStreamOutcome::Done(ChatDone { result, todos }));
                }

                // Round-trip the assistant's tool-call turn back into history
                // so the next request shows the provider its own prior
                // request. `None` content for a tool-only turn matches the
                // wire reality (`LlmMessage::content`'s own doc comment).
                history.push(LlmMessage {
                    role: LlmRole::Assistant,
                    content: if result.text.is_empty() { None } else { Some(result.text.clone()) },
                    tool_call_id: None,
                    tool_calls: sanitize_tool_call_arguments(&result.tool_calls),
                });

                // Charged before the pause check below, so a round that
                // immediately pauses for approval is still billed — see the
                // matching comment on the resumed branch above.
                budget_used += round_cost(&result.tool_calls);

                let pending: Vec<PendingToolCall> = result
                    .tool_calls
                    .iter()
                    .map(|call| PendingToolCall {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                        requires_confirmation: ToolName::from_wire_name(&call.name)
                            .is_some_and(ToolName::requires_confirmation),
                    })
                    .collect();
                if pending.iter().any(|c| c.requires_confirmation) {
                    return Ok(ChatStreamOutcome::PendingApproval(PendingApproval {
                        history,
                        round,
                        budget_used,
                        calls: pending,
                        todos,
                    }));
                }

                (result.tool_calls, Vec::new())
            };

        let log_ctx = ToolCallLogContext {
            enabled: ctx.settings.tool_call_logging,
            source: "chat",
            round: Some(round),
            provider_id: Some(ctx.provider_id.to_string()),
            model: Some(ctx.model.to_string()),
        };
        for call in &tool_calls {
            // Checkpoint 1's "between individual tool calls" case — `break`
            // rather than returning directly so control falls through to
            // the top of the outer `loop`, where checkpoint 1 itself
            // resolves `Cancelled` (one place that builds that outcome,
            // not two).
            if ctx.cancel_flag.load(Ordering::SeqCst) {
                break;
            }
            let _ = ctx.app.emit(
                TOOL_CALL_EVENT,
                ToolCallEventPayload {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                },
            );

            // A bad tool call (unknown name, malformed arguments, a
            // NotAllowed hit against the allowlist, a missing file, ...) is
            // always recoverable-by-the-model, never a hard failure of the
            // whole turn — same for a user-denied call, which is just
            // another kind of "this didn't happen, react accordingly."
            let denied = decisions.iter().any(|d| d.id == call.id && !d.approved);
            let outcome: Result<ToolResult, String> = if denied {
                Err("denied by user".to_string())
            } else {
                ai_tools::parse_tool_call(call)
                    .and_then(|parsed| ai_tools::execute_tool_logged(&scope, parsed, ctx.deps, &todos, &log_ctx))
                    .map_err(|e| e.to_string())
            };

            let _ = ctx.app.emit(
                TOOL_RESULT_EVENT,
                ToolResultEventPayload {
                    id: call.id.clone(),
                    result: outcome.as_ref().ok().cloned(),
                    error: outcome.as_ref().err().cloned(),
                },
            );

            // A successful RequestFullRepoAccess must take effect for the
            // rest of THIS turn, not just the next `llm_chat_stream` call —
            // see this function's doc comment.
            if let Ok(ToolResult::AccessModeChanged { .. }) = &outcome {
                if let Ok(new_scope) = ai_tools::current_scope() {
                    tools = ai_tools::llm_tool_definitions(&new_scope);
                    scope = new_scope;
                }
            }

            // A successful `todo` call's result carries the *complete* new
            // list — overwrite this loop's own `todos` so subsequent calls
            // in this round (or a later round) see it, and so it's what
            // ultimately lands in `ChatStreamOutcome`. Same pattern as the
            // `AccessModeChanged` handling above: read the outcome once it
            // settles, update loop-scoped state that outlives this one call.
            match &outcome {
                Ok(ToolResult::TodoWritten(list)) | Ok(ToolResult::TodoUpdated(list)) => {
                    todos = list.clone();
                }
                _ => {}
            }

            // Text the *model* reads for this call, as opposed to `outcome`
            // itself (also emitted verbatim as `TOOL_RESULT_EVENT.error` for
            // the UI, which pattern-matches the literal `"denied by user"`
            // string from above — see `describeToolResult` in
            // `assistantConfig.ts`). Kept in Russian here, independently of
            // that English marker, so a tool failure or a denied call
            // doesn't hand the model a chunk of English prose to continue
            // from mid-turn.
            let content = match &outcome {
                // Rendered as an ASCII tree rather than the raw JSON array
                // every other `ToolResult` gets below — a flat list of
                // `{path, isDir}` objects forces the model to reconstruct
                // the directory structure itself from N separate paths; a
                // tree hands it the whole shape (and where each entry sits)
                // at a glance, same as a human skimming `tree(1)` output.
                Ok(ToolResult::FileList(entries)) => {
                    let root_label = scope
                        .root
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(".");
                    ai_tools::render_file_tree(entries, root_label)
                }
                Ok(tool_result) => serde_json::to_string(tool_result)
                    .unwrap_or_else(|_| "Ошибка: не удалось сериализовать результат инструмента".to_string()),
                Err(e) if e == "denied by user" => "Отклонено пользователем".to_string(),
                Err(e) => format!("Ошибка: {e}"),
            };
            history.push(LlmMessage {
                role: LlmRole::Tool,
                content: Some(content),
                tool_call_id: Some(call.id.clone()),
                tool_calls: vec![],
            });
        }
    }
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
    tauri::async_runtime::spawn_blocking(move || -> Result<ChatStreamOutcome, String> {
        let settings = llm_config::load_llm_settings().map_err(|e| e.to_string())?;
        let resolved =
            llm_config::resolve_provider(&provider_id, &settings).map_err(|e| e.to_string())?;
        let api_key = llm_credentials_store::get_api_key(&provider_id);
        let provider = ensure_llm_provider(&llm_provider, &resolved, api_key)?;
        let model = llm_config::effective_model(&resolved, provider.as_ref())
            .map_err(|e| e.to_string())?;
        deps.fast_apply = Some((provider.clone(), model.clone()));

        // No project open is not something the model can recover from by
        // trying again — hard-fail the whole command, same as
        // `ai_execute_tool` does for the same condition.
        let scope = ai_tools::current_scope().map_err(|e| e.to_string())?;
        deps.active_file = resolve_active_file(&scope, active_file_path);
        let tools = ai_tools::llm_tool_definitions(&scope);

        let ctx = LoopCtx {
            app: &app,
            provider: provider.as_ref(),
            provider_id: &provider_id,
            model: &model,
            settings: &settings,
            deps: &deps,
            cancel_flag: &cancel_flag,
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
    tauri::async_runtime::spawn_blocking(move || -> Result<ChatStreamOutcome, String> {
        let settings = llm_config::load_llm_settings().map_err(|e| e.to_string())?;
        let resolved =
            llm_config::resolve_provider(&provider_id, &settings).map_err(|e| e.to_string())?;
        let api_key = llm_credentials_store::get_api_key(&provider_id);
        let provider = ensure_llm_provider(&llm_provider, &resolved, api_key)?;
        let model = llm_config::effective_model(&resolved, provider.as_ref())
            .map_err(|e| e.to_string())?;
        deps.fast_apply = Some((provider.clone(), model.clone()));

        let scope = ai_tools::current_scope().map_err(|e| e.to_string())?;
        deps.active_file = resolve_active_file(&scope, active_file_path);
        let tools = ai_tools::llm_tool_definitions(&scope);

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
            .filter(|c| ToolName::from_wire_name(&c.name).is_some_and(ToolName::requires_confirmation))
            .map(|c| c.id.as_str())
            .collect();
        let provided: HashSet<&str> = decisions.iter().map(|d| d.id.as_str()).collect();
        if expected != provided {
            return Err(
                "resume: decisions do not match this round's pending calls".to_string()
            );
        }

        let ctx = LoopCtx {
            app: &app,
            provider: provider.as_ref(),
            provider_id: &provider_id,
            model: &model,
            settings: &settings,
            deps: &deps,
            cancel_flag: &cancel_flag,
        };
        run_tool_loop(&ctx, scope, tools, history, round, budget_used, Some((calls, decisions)), todos)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str) -> LlmToolCall {
        LlmToolCall { id: "1".to_string(), name: name.to_string(), arguments: "{}".to_string() }
    }

    #[test]
    fn round_cost_of_no_calls_is_zero() {
        assert_eq!(round_cost(&[]), 0);
    }

    #[test]
    fn round_cost_sums_weights_of_every_call_in_the_round() {
        // `readFile` (1) + `writeFile` (2) + `semanticSearch` (4) bundled
        // into one round, mirroring how a model can request several
        // parallel calls in a single completion.
        let calls = [call("readFile"), call("writeFile"), call("semanticSearch")];
        assert_eq!(round_cost(&calls), 7);
    }

    #[test]
    fn round_cost_of_an_unrecognized_tool_name_floors_to_one() {
        // A hallucinated/unknown tool name must still make forward
        // progress against the budget, same as the cheapest real tool.
        assert_eq!(round_cost(&[call("notARealTool")]), 1);
    }
}
