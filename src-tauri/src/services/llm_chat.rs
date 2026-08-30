//! The agentic chat loop: advertise the project's allowed tools, call the
//! model, execute whatever it asks for, feed the results back, repeat — until
//! a round produces no more tool calls, a round needs user confirmation, the
//! user stops the turn, or the loop runs out of budget.
//!
//! Split out of `commands::llm`, where it was 335 lines of orchestration
//! sitting inside the IPC boundary. Nothing here knows about Tauri: progress
//! is reported through a `ChatEventSink`, the same shape
//! `embedding_sync::ProgressSink`, `IndexWatcher::start` and
//! `domain::llm::LlmProvider::chat_stream` already use — except that a chat
//! turn reports five different kinds of thing, so the sink carries a
//! `domain::llm::ChatEvent` enum rather than one payload type.
//!
//! Provider resolution and the resident provider cache live next door in
//! `services::llm_session`.

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::domain::ai_access::{call_requires_confirmation, ToolName};
use crate::domain::ai_tools::{Task, ToolResult, ToolScope};
use crate::domain::conversation_mode::{mode_tools, ConversationMode};
use crate::domain::llm::{
    sanitize_tool_call_arguments, ChatDone, ChatEvent, ChatEventSink, ChatRequest,
    ChatStreamDelta,
    ChatStreamResult,
    ChatStreamOutcome, ChatStreamReasoning, LlmMessage, LlmProvider, LlmRole, LlmSettings,
    LlmToolCall, LlmToolDefinition, PendingApproval, PendingToolCall, ToolCallDecision,
    SteeringAppliedEvent, ToolCallEvent, ToolResultEvent, STEERING_PREFIX,
};
use crate::domain::paths;
use crate::domain::repo_index::FileId;
use crate::infra::llm_debug_log;
use crate::services::ai_tools::{self, EmbeddingDeps, ToolCallLogContext};
use crate::services::llm_rate_limit;
use crate::services::llm_session;
use crate::services::llm_session::{ChatCancelFlag, LlmProviderSlot, SteeringQueue};


/// A misbehaving/looping model shouldn't be able to hold the UI in a
/// "thinking" state indefinitely — this caps how many model↔tool round
/// trips one `llm_chat_stream` call will run before hard-failing. Kept as
/// a backstop alongside `MAX_TOOL_BUDGET` (a misconfigured/zero tool
/// weight must never make the loop unstoppable), but `MAX_TOOL_BUDGET` is
/// the more sensitive limit in practice — see its doc comment.
const MAX_TOOL_ITERATIONS: usize = 60;

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
    events: &'a ChatEventSink,
    provider: &'a dyn LlmProvider,
    provider_id: &'a str,
    model: &'a str,
    settings: &'a LlmSettings,
    deps: &'a EmbeddingDeps,
    cancel_flag: &'a ChatCancelFlag,
    steering: &'a SteeringQueue,
    /// Pinned for the whole call — unlike `scope`/`tools` (which
    /// `RequestFullRepoAccess` widens mid-loop), a `RequestModeSwitch`
    /// deliberately does *not* take effect within the same turn (see
    /// `domain::conversation_mode`'s doc comment and `services::ai_tools::
    /// execute_tool`'s `RequestModeSwitch` arm) — so this never changes for
    /// the lifetime of one `run_tool_loop` call.
    conversation_mode: ConversationMode,
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
/// already known) contains any call `call_requires_confirmation` flags.
/// Nothing in that round executes — not even other, non-risky calls bundled
/// into the same round — so there's no partial-round state to track across
/// the stateless hop back to the frontend.
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
                result: ChatStreamResult { text: String::new(), reasoning: String::new(), usage: None, tool_calls: vec![] },
                todos,
            }));
        }
        if round >= MAX_TOOL_ITERATIONS as u32 || budget_used >= MAX_TOOL_BUDGET {
            return Err(format!(
                "Ассистент не дал окончательный ответ за {MAX_TOOL_ITERATIONS} раундов обращения к инструментам. Попросите ассистента продолжить, если вы уверены, что он ещё не закончил работу."
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
                let notes: Vec<String> = ctx
                    .steering
                    .lock()
                    .map_err(|_| "steering queue lock poisoned".to_string())?
                    .drain(..)
                    .collect();
                for note in notes {
                    history.push(LlmMessage {
                        role: LlmRole::User,
                        content: Some(format!("{STEERING_PREFIX}{note}")),
                        tool_call_id: None,
                        tool_calls: vec![],
                    });
                    (ctx.events)(ChatEvent::SteeringApplied(SteeringAppliedEvent { text: note }));
                }
                let request = ChatRequest {
                    messages: history.clone(),
                    tools: tools.clone(),
                    model: ctx.model.to_string(),
                };
                llm_debug_log::log_request(ctx.settings.debug_logging, ctx.provider_id, round, &request);
                let on_delta = |delta: &str| {
                    (ctx.events)(ChatEvent::Delta(ChatStreamDelta {
                        delta: delta.to_string(),
                    }));
                };
                let on_reasoning = |delta: &str| {
                    (ctx.events)(ChatEvent::Reasoning(ChatStreamReasoning {
                        delta: delta.to_string(),
                    }));
                };
                let on_tool_call_delta = |id: &str, name: &str, arguments: &str| {
                    (ctx.events)(ChatEvent::ToolCallDelta(ToolCallEvent {
                        id: id.to_string(),
                        name: name.to_string(),
                        arguments: arguments.to_string(),
                    }));
                };
                let cancelled = || ctx.cancel_flag.load(Ordering::SeqCst);
                let raw_result = ctx
                    .provider
                    .chat_stream(request, &on_delta, &on_reasoning, &on_tool_call_delta, &cancelled);
                llm_debug_log::log_response(ctx.settings.debug_logging, ctx.provider_id, round, &raw_result);
                let result = raw_result.map_err(|e| e.to_string())?;
                if let Some(usage) = result.usage {
                    llm_rate_limit::record(ctx.provider_id, usage.completion_tokens);
                    (ctx.events)(ChatEvent::RateLimitChanged);
                    (ctx.events)(ChatEvent::ContextUsage(usage));
                }

                // Checkpoint 2 — see this function's doc comment. Checked
                // before either branch below so a stop that landed exactly
                // as this round finished (mid-stream, or naturally) never
                // reaches the pending-approval check or executes any of
                // this round's tool calls, confirmation-gated or not.
                if cancelled() {
                    return Ok(ChatStreamOutcome::Cancelled(ChatDone { result, todos }));
                }

                if result.tool_calls.is_empty() {
                    let has_pending_steering = !ctx
                        .steering
                        .lock()
                        .map_err(|_| "steering queue lock poisoned".to_string())?
                        .is_empty();
                    if !has_pending_steering {
                        return Ok(ChatStreamOutcome::Done(ChatDone { result, todos }));
                    }
                    history.push(LlmMessage {
                        role: LlmRole::Assistant,
                        content: if result.text.is_empty() { None } else { Some(result.text) },
                        tool_call_id: None,
                        tool_calls: vec![],
                    });
                    continue;
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

                // Path-containment preflight before any approval UI: a write
                // outside the documentation root must fail as a tool error
                // immediately, not show a confirmation card for an impossible
                // operation. Successful calls stay in `remaining` for the
                // normal confirm-or-execute path.
                let mut remaining_calls: Vec<LlmToolCall> = Vec::new();
                for call in &result.tool_calls {
                    if let Err(e) = ai_tools::preflight_tool_call(&scope, call) {
                        (ctx.events)(ChatEvent::ToolCall(ToolCallEvent {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                        }));
                        let err_str = e.to_string();
                        (ctx.events)(ChatEvent::ToolResult(ToolResultEvent {
                            id: call.id.clone(),
                            result: None,
                            error: Some(err_str.clone()),
                        }));
                        history.push(LlmMessage {
                            role: LlmRole::Tool,
                            content: Some(format!("Ошибка: {err_str}")),
                            tool_call_id: Some(call.id.clone()),
                            tool_calls: vec![],
                        });
                    } else {
                        remaining_calls.push(call.clone());
                    }
                }

                if remaining_calls.is_empty() {
                    // Every call in this round failed preflight — no card,
                    // let the model react to the tool errors on the next
                    // round.
                    continue;
                }

                let pending: Vec<PendingToolCall> = remaining_calls
                    .iter()
                    .map(|call| PendingToolCall {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                        requires_confirmation: call_requires_confirmation(
                            &call.name,
                            &call.arguments,
                        ),
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

                (remaining_calls, Vec::new())
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
            (ctx.events)(ChatEvent::ToolCall(ToolCallEvent {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            }));

            // A bad tool call (unknown name, malformed arguments, a
            // NotAllowed hit against the allowlist, a missing file, ...) is
            // always recoverable-by-the-model, never a hard failure of the
            // whole turn — same for a user-denied call, which is just
            // another kind of "this didn't happen, react accordingly."
            //
            // `askUser` and `requestArtifact` are special: their results
            // come from the user's `ToolCallDecision`, not from
            // `execute_tool` (which rejects a bare call for either). Skip →
            // same denial path as Approve/Deny tools.
            let decision = decisions.iter().find(|d| d.id == call.id);
            let denied = decision.map(|d| !d.approved).unwrap_or(false);
            let outcome: Result<ToolResult, String> = if denied {
                Err("denied by user".to_string())
            } else if call.name == "requestArtifact" {
                if !mode_tools(ctx.conversation_mode).contains(&ToolName::RequestArtifact) {
                    Err(format!(
                        "tool '{}' is not available in the current conversation mode",
                        call.name
                    ))
                } else {
                    // The decision carries only an id — the record itself is
                    // read from the store here, so the model cannot be handed
                    // an artifact that differs from the saved one.
                    match decision.and_then(|d| d.artifact_id.as_deref()) {
                        Some(artifact_id) => crate::services::artifacts::get(artifact_id)
                            .map(ai_tools::artifact_result)
                            .map_err(|e| e.to_string()),
                        None => Err("denied by user".to_string()),
                    }
                }
            } else if call.name == "askUser" {
                if !mode_tools(ctx.conversation_mode).contains(&ToolName::AskUser) {
                    Err(format!(
                        "tool '{}' is not available in the current conversation mode",
                        call.name
                    ))
                } else {
                    match decision.and_then(|d| d.answer.as_ref()) {
                        Some(payload) => Ok(ToolResult::AskUser {
                            answers: payload.answers.clone(),
                        }),
                        None => Err("denied by user".to_string()),
                    }
                }
            } else {
                ai_tools::parse_tool_call(call).map_err(|e| e.to_string()).and_then(|parsed| {
                    // Defense in depth alongside `llm_tool_definitions`'s own
                    // mode-aware filtering (see `commands::ai_tools::
                    // ai_get_tool_definitions`/this function's `tools`
                    // param) — the model shouldn't be *offered* a tool
                    // outside `ctx.conversation_mode`, but this catches it
                    // regardless (a hallucinated name, or a call queued
                    // before a since-changed mode).
                    if !mode_tools(ctx.conversation_mode).contains(&parsed.name()) {
                        return Err(format!(
                            "tool '{}' is not available in the current conversation mode",
                            call.name
                        ));
                    }
                    ai_tools::execute_tool_logged(&scope, parsed, ctx.deps, &todos, &log_ctx)
                        .map_err(|e| e.to_string())
                })
            };

            (ctx.events)(ChatEvent::ToolResult(ToolResultEvent {
                id: call.id.clone(),
                result: outcome.as_ref().ok().cloned(),
                error: outcome.as_ref().err().cloned(),
            }));

            // A successful RequestFullRepoAccess must take effect for the
            // rest of THIS turn, not just the next `llm_chat_stream` call —
            // see this function's doc comment.
            if let Ok(ToolResult::AccessModeChanged { .. }) = &outcome {
                if let Ok(new_scope) = ai_tools::current_scope() {
                    tools = ai_tools::llm_tool_definitions(&new_scope, ctx.conversation_mode);
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
                    ai_tools::render_file_tree(entries)
                }
                // Skip path for the two pause-only tools — Russian, matching
                // the deny message for mutating tools so the model continues
                // in-language. For `requestArtifact` this is «Заполню позже»:
                // the user did not refuse the work, only deferred it, so the
                // model should carry on and say what is still missing.
                Err(e)
                    if e == "denied by user"
                        && (call.name == "askUser" || call.name == "requestArtifact") =>
                {
                    "Пропущено пользователем".to_string()
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


/// Everything one turn needs that is not the conversation itself.
/// `commands::llm` assembles this from `tauri::State`, so neither use-case
/// below grows a nine-parameter signature that has to stay in the same order
/// at two call sites.
pub struct ChatTurnContext {
    pub provider_slot: Arc<LlmProviderSlot>,
    pub cancel_flag: Arc<ChatCancelFlag>,
    pub steering: Arc<SteeringQueue>,
    /// `fast_apply` and `active_file` are filled in here, once the provider
    /// and scope are resolved — callers leave them `None`.
    pub deps: EmbeddingDeps,
    pub provider_id: String,
    pub active_file_path: Option<String>,
    pub conversation_mode: ConversationMode,
}

/// The checkpoint a `ChatStreamOutcome::PendingApproval` handed the frontend,
/// sent back verbatim. The backend keeps no session state between calls, so
/// this is the entire resumable state of a paused turn.
pub struct ResumePoint {
    pub history: Vec<LlmMessage>,
    pub round: u32,
    pub budget_used: u32,
    pub decisions: Vec<ToolCallDecision>,
    pub todos: Vec<Task>,
}

/// Resolved once per turn: provider, model, scope, advertised tools.
struct TurnSetup {
    provider: Arc<dyn LlmProvider>,
    model: String,
    settings: LlmSettings,
    scope: ToolScope,
    tools: Vec<LlmToolDefinition>,
}

fn setup(ctx: &mut ChatTurnContext) -> Result<TurnSetup, String> {
    let llm_session::LlmSession { provider, model, settings, .. } =
        llm_session::resolve(&ctx.provider_id, &ctx.provider_slot)?;
    // `EditFile`'s fast-apply fallback reuses the exact provider/model this
    // turn is already using for chat, rather than resolving a second one.
    ctx.deps.fast_apply = Some((provider.clone(), model.clone()));

    // No project open is not something the model can recover from by trying
    // again — hard-fail the whole turn, same as `ai_execute_tool` does.
    let scope = ai_tools::current_scope().map_err(|e| e.to_string())?;
    ctx.deps.active_file = resolve_active_file(&scope, ctx.active_file_path.take());
    let tools = ai_tools::llm_tool_definitions(&scope, ctx.conversation_mode);

    Ok(TurnSetup { provider, model, settings, scope, tools })
}

/// A fresh conversation turn. Runs the tool-calling loop from round zero and
/// resolves once the model stops asking for tools, a round needs
/// confirmation, or the turn is cancelled.
pub fn stream(
    mut ctx: ChatTurnContext,
    messages: Vec<LlmMessage>,
    todos: Vec<Task>,
    events: &ChatEventSink,
) -> Result<ChatStreamOutcome, String> {
    // A *fresh* turn always starts with a clean flag — a stray cancel from an
    // already-finished previous turn must never bleed into this one.
    // `stream_resume` deliberately does not do this (see `ChatCancelFlag`).
    ctx.cancel_flag.store(false, Ordering::SeqCst);
    ctx.steering
        .lock()
        .map_err(|_| "steering queue lock poisoned".to_string())?
        .clear();

    let setup = setup(&mut ctx)?;
    let loop_ctx = LoopCtx {
        events,
        provider: setup.provider.as_ref(),
        provider_id: &ctx.provider_id,
        model: &setup.model,
        settings: &setup.settings,
        deps: &ctx.deps,
        cancel_flag: &ctx.cancel_flag,
        steering: &ctx.steering,
        conversation_mode: ctx.conversation_mode,
    };
    run_tool_loop(&loop_ctx, setup.scope, setup.tools, messages, 0, 0, None, todos)
}

/// Continues a turn paused by `PendingApproval`. `resume` must be exactly
/// what that outcome carried: the history must still end with the assistant's
/// tool-call turn, and the decisions must cover exactly the calls that needed
/// confirmation — anything else is rejected up front rather than silently
/// executing calls the user never actually saw.
pub fn stream_resume(
    mut ctx: ChatTurnContext,
    resume: ResumePoint,
    events: &ChatEventSink,
) -> Result<ChatStreamOutcome, String> {
    let setup = setup(&mut ctx)?;

    let ResumePoint { history, round, budget_used, decisions, todos } = resume;
    let last = history
        .last()
        .ok_or_else(|| "resume: history must not be empty".to_string())?;
    if last.role != LlmRole::Assistant || last.tool_calls.is_empty() {
        return Err("resume: history must end with the assistant's tool-call turn".to_string());
    }
    let calls = last.tool_calls.clone();

    let expected: HashSet<&str> = calls
        .iter()
        .filter(|c| call_requires_confirmation(&c.name, &c.arguments))
        .map(|c| c.id.as_str())
        .collect();
    let provided: HashSet<&str> = decisions.iter().map(|d| d.id.as_str()).collect();
    if expected != provided {
        return Err("resume: decisions do not match this round's pending calls".to_string());
    }

    let loop_ctx = LoopCtx {
        events,
        provider: setup.provider.as_ref(),
        provider_id: &ctx.provider_id,
        model: &setup.model,
        settings: &setup.settings,
        deps: &ctx.deps,
        cancel_flag: &ctx.cancel_flag,
        steering: &ctx.steering,
        conversation_mode: ctx.conversation_mode,
    };
    run_tool_loop(
        &loop_ctx,
        setup.scope,
        setup.tools,
        history,
        round,
        budget_used,
        Some((calls, decisions)),
        todos,
    )
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

    // --- `run_tool_loop` ---

    use crate::domain::ai_access::{default_allowed_tools, AiAccessMode};
    use crate::domain::llm::{ChatResponse, ChatStreamResult, ChatUsage, LlmError, LlmModelInfo};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU64, AtomicUsize};
    use std::sync::Mutex;

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture_repo(label: &str) -> std::path::PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("alfa-atlas-llm-chat-{label}-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("intro.adoc"), "= Intro\n\nHello.\n").unwrap();
        dir
    }

    /// Hands back one programmed round per `chat_stream` call. `fallback` is
    /// repeated once the script runs out, so a test can assert termination
    /// without scripting sixty rounds by hand.
    struct ScriptedProvider {
        rounds: Mutex<VecDeque<ChatStreamResult>>,
        fallback: Option<ChatStreamResult>,
        calls: AtomicUsize,
        /// Every request the loop sent, in order — the only way to observe
        /// what a settled tool call actually handed back to the model, since
        /// `ChatDone` carries the answer but not the history behind it.
        requests: Mutex<Vec<ChatRequest>>,
        steer_after_call: Option<(usize, Arc<SteeringQueue>, String)>,
    }

    impl ScriptedProvider {
        fn new(rounds: Vec<ChatStreamResult>) -> Self {
            Self {
                rounds: Mutex::new(rounds.into()),
                fallback: None,
                calls: AtomicUsize::new(0),
                requests: Mutex::new(Vec::new()),
                steer_after_call: None,
            }
        }

        fn repeating(round: ChatStreamResult) -> Self {
            Self {
                rounds: Mutex::new(VecDeque::new()),
                fallback: Some(round),
                calls: AtomicUsize::new(0),
                requests: Mutex::new(Vec::new()),
                steer_after_call: None,
            }
        }

        fn steering_after_call(mut self, call: usize, queue: Arc<SteeringQueue>, text: &str) -> Self {
            self.steer_after_call = Some((call, queue, text.to_string()));
            self
        }

        /// The `Tool`-role content the model read back for `call_id`.
        fn tool_reply(&self, call_id: &str) -> String {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .flat_map(|r| r.messages.iter())
                .find(|m| m.role == LlmRole::Tool && m.tool_call_id.as_deref() == Some(call_id))
                .and_then(|m| m.content.clone())
                .unwrap_or_default()
        }
    }

    impl LlmProvider for ScriptedProvider {
        fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, LlmError> {
            unimplemented!("the tool loop only ever calls chat_stream()")
        }

        fn chat_stream(
            &self,
            _request: ChatRequest,
            on_delta: &dyn Fn(&str),
            _on_reasoning: &dyn Fn(&str),
            _on_tool_call_delta: &dyn Fn(&str, &str, &str),
            _cancelled: &dyn Fn() -> bool,
        ) -> Result<ChatStreamResult, LlmError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            self.requests.lock().unwrap().push(_request);
            if let Some((target, queue, text)) = &self.steer_after_call {
                if call == *target {
                    queue.lock().unwrap().push(text.clone());
                }
            }
            let next = self.rounds.lock().unwrap().pop_front();
            let round = match (next, &self.fallback) {
                (Some(r), _) => r,
                (None, Some(f)) => f.clone(),
                (None, None) => panic!("the loop asked for more rounds than the test scripted"),
            };
            if !round.text.is_empty() {
                on_delta(&round.text);
            }
            Ok(round)
        }

        fn list_models(&self) -> Result<Vec<LlmModelInfo>, LlmError> {
            unimplemented!("the tool loop never lists models")
        }
    }

    fn round(text: &str, calls: Vec<LlmToolCall>) -> ChatStreamResult {
        ChatStreamResult {
            text: text.to_string(),
            reasoning: String::new(),
            usage: None,
            tool_calls: calls,
        }
    }

    fn tool_call(id: &str, name: &str, arguments: &str) -> LlmToolCall {
        LlmToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        }
    }

    fn collector() -> (ChatEventSink, Arc<Mutex<Vec<ChatEvent>>>) {
        let seen: Arc<Mutex<Vec<ChatEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_target = seen.clone();
        let sink: ChatEventSink = Arc::new(move |e| sink_target.lock().unwrap().push(e));
        (sink, seen)
    }

    /// Drives `run_tool_loop` directly — no open project, no `$HOME`, no
    /// Tauri runtime. Everything the loop branches on is a parameter.
    fn run(
        provider: &dyn LlmProvider,
        root: &std::path::Path,
        events: &ChatEventSink,
        cancel_flag: &ChatCancelFlag,
    ) -> Result<ChatStreamOutcome, String> {
        let steering = SteeringQueue::default();
        run_with_steering(provider, root, events, cancel_flag, &steering)
    }

    fn run_with_steering(
        provider: &dyn LlmProvider,
        root: &std::path::Path,
        events: &ChatEventSink,
        cancel_flag: &ChatCancelFlag,
        steering: &SteeringQueue,
    ) -> Result<ChatStreamOutcome, String> {
        let scope = ToolScope::new(
            root,
            root,
            AiAccessMode::FullRepo,
            default_allowed_tools(AiAccessMode::FullRepo),
        );
        let settings = LlmSettings::default();
        let deps = EmbeddingDeps::empty();
        let ctx = LoopCtx {
            events,
            provider,
            provider_id: "test-provider",
            model: "test-model",
            settings: &settings,
            deps: &deps,
            cancel_flag,
            steering,
            conversation_mode: ConversationMode::Agent,
        };
        run_tool_loop(&ctx, scope, Vec::new(), Vec::new(), 0, 0, None, Vec::new())
    }

    /// `run`, re-entered on a paused round — the shape
    /// `llm_chat_stream_resume` produces. `history` must end with the
    /// assistant's tool-call turn, same as the real resume path checks.
    fn run_resumed(
        provider: &dyn LlmProvider,
        root: &std::path::Path,
        events: &ChatEventSink,
        cancel_flag: &ChatCancelFlag,
        calls: Vec<LlmToolCall>,
        decisions: Vec<ToolCallDecision>,
    ) -> Result<ChatStreamOutcome, String> {
        let steering = SteeringQueue::default();
        run_resumed_with_steering(provider, root, events, cancel_flag, &steering, calls, decisions)
    }

    fn run_resumed_with_steering(
        provider: &dyn LlmProvider,
        root: &std::path::Path,
        events: &ChatEventSink,
        cancel_flag: &ChatCancelFlag,
        steering: &SteeringQueue,
        calls: Vec<LlmToolCall>,
        decisions: Vec<ToolCallDecision>,
    ) -> Result<ChatStreamOutcome, String> {
        let scope = ToolScope::new(
            root,
            root,
            AiAccessMode::FullRepo,
            default_allowed_tools(AiAccessMode::FullRepo),
        );
        let settings = LlmSettings::default();
        let deps = EmbeddingDeps::empty();
        let ctx = LoopCtx {
            events,
            provider,
            provider_id: "test-provider",
            model: "test-model",
            settings: &settings,
            deps: &deps,
            cancel_flag,
            steering,
            conversation_mode: ConversationMode::Agent,
        };
        let history = vec![LlmMessage {
            role: LlmRole::Assistant,
            content: None,
            tool_call_id: None,
            tool_calls: calls.clone(),
        }];
        run_tool_loop(
            &ctx,
            scope,
            Vec::new(),
            history,
            1,
            0,
            Some((calls, decisions)),
            Vec::new(),
        )
    }


    fn tool_events(seen: &Arc<Mutex<Vec<ChatEvent>>>) -> Vec<(String, String)> {
        seen.lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                ChatEvent::ToolCall(c) => Some(("call".to_string(), c.id.clone())),
                ChatEvent::ToolResult(r) => Some(("result".to_string(), r.id.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_round_with_no_tool_calls_resolves_immediately() {
        let root = fixture_repo("plain");
        let provider = ScriptedProvider::new(vec![round("Готово.", vec![])]);
        let (events, seen) = collector();
        let cancel = ChatCancelFlag::new(false);

        let outcome = run(&provider, &root, &events, &cancel).unwrap();

        let ChatStreamOutcome::Done(done) = outcome else {
            panic!("expected Done, got something else");
        };
        assert_eq!(done.result.text, "Готово.");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1, "one round, one model call");
        assert!(tool_events(&seen).is_empty(), "nothing was executed");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_tool_round_executes_then_the_next_round_finishes_the_turn() {
        let root = fixture_repo("tool-round");
        let provider = ScriptedProvider::new(vec![
            round("", vec![tool_call("c1", "readFile", r#"{"path":"intro.adoc"}"#)]),
            round("Прочитал.", vec![]),
        ]);
        let (events, seen) = collector();
        let cancel = ChatCancelFlag::new(false);

        let outcome = run(&provider, &root, &events, &cancel).unwrap();

        assert!(matches!(outcome, ChatStreamOutcome::Done(_)));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2, "tool round + answer round");
        // The frontend pairs a call with its result by id (`chatBlocks.ts`),
        // so both the pairing and the order are contract, not incidental.
        assert_eq!(
            tool_events(&seen),
            vec![
                ("call".to_string(), "c1".to_string()),
                ("result".to_string(), "c1".to_string()),
            ]
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn queued_steering_is_applied_to_the_next_fresh_round() {
        let root = fixture_repo("steering");
        let steering = Arc::new(SteeringQueue::default());
        let provider = ScriptedProvider::new(vec![
            round("", vec![tool_call("c1", "readFile", r#"{"path":"intro.adoc"}"#)]),
            round("Готово.", vec![]),
        ])
        .steering_after_call(1, steering.clone(), "Проверь ru locale");
        let (events, seen) = collector();
        let cancel = ChatCancelFlag::new(false);

        let outcome =
            run_with_steering(&provider, &root, &events, &cancel, steering.as_ref()).unwrap();

        assert!(matches!(outcome, ChatStreamOutcome::Done(_)));
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(!requests[0].messages.iter().any(|m| {
            m.role == LlmRole::User
                && m.content.as_deref() == Some(&format!("{STEERING_PREFIX}Проверь ru locale"))
        }));
        assert!(requests[1].messages.iter().any(|m| {
            m.role == LlmRole::User
                && m.content.as_deref() == Some(&format!("{STEERING_PREFIX}Проверь ru locale"))
        }));
        assert!(seen.lock().unwrap().iter().any(|event| matches!(
            event,
            ChatEvent::SteeringApplied(SteeringAppliedEvent { text })
                if text == "Проверь ru locale"
        )));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn steering_during_a_final_stream_creates_a_follow_up_round() {
        let root = fixture_repo("steering-final");
        let steering = Arc::new(SteeringQueue::default());
        let provider = ScriptedProvider::new(vec![
            round("Первый ответ.", vec![]),
            round("Ответ с уточнением.", vec![]),
        ])
        .steering_after_call(1, steering.clone(), "Проверь ru locale");
        let (events, _) = collector();
        let cancel = ChatCancelFlag::new(false);

        let outcome =
            run_with_steering(&provider, &root, &events, &cancel, steering.as_ref()).unwrap();

        let ChatStreamOutcome::Done(done) = outcome else {
            panic!("expected Done");
        };
        assert_eq!(done.result.text, "Ответ с уточнением.");
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].messages.iter().any(|message| {
            message.role == LlmRole::User
                && message.content.as_deref()
                    == Some(&format!("{STEERING_PREFIX}Проверь ru locale"))
        }));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn steering_queued_during_approval_waits_for_post_resume_round() {
        let root = fixture_repo("steering-approval");
        let steering = Arc::new(SteeringQueue::default());
        let call = tool_call("write", "writeFile", r#"{"path":"new.adoc","content":"x"}"#);
        let provider = ScriptedProvider::new(vec![
            round("", vec![call.clone()]),
            round("Изменение выполнено.", vec![]),
        ])
        .steering_after_call(1, steering.clone(), "Добавь заголовок");
        let (events, _) = collector();
        let cancel = ChatCancelFlag::new(false);

        let paused =
            run_with_steering(&provider, &root, &events, &cancel, steering.as_ref()).unwrap();
        assert!(matches!(paused, ChatStreamOutcome::PendingApproval(_)));
        assert_eq!(provider.requests.lock().unwrap().len(), 1);

        let resumed = run_resumed_with_steering(
            &provider,
            &root,
            &events,
            &cancel,
            steering.as_ref(),
            vec![call],
            vec![ToolCallDecision {
                id: "write".to_string(),
                approved: true,
                answer: None,
                artifact_id: None,
            }],
        )
        .unwrap();

        assert!(matches!(resumed, ChatStreamOutcome::Done(_)));
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 2, "resume executes calls before making a fresh request");
        assert!(requests[1].messages.iter().any(|m| {
            m.role == LlmRole::User
                && m.content.as_deref() == Some(&format!("{STEERING_PREFIX}Добавь заголовок"))
        }));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_round_needing_confirmation_pauses_without_executing_anything() {
        let root = fixture_repo("approval");
        // A confirmation-gated call bundled with a harmless one: neither may
        // run, or the user would be approving something already done.
        let provider = ScriptedProvider::new(vec![round(
            "",
            vec![
                tool_call("safe", "readFile", r#"{"path":"intro.adoc"}"#),
                tool_call("risky", "writeFile", r#"{"path":"new.adoc","content":"x"}"#),
            ],
        )]);
        let (events, seen) = collector();
        let cancel = ChatCancelFlag::new(false);

        let outcome = run(&provider, &root, &events, &cancel).unwrap();

        let ChatStreamOutcome::PendingApproval(pending) = outcome else {
            panic!("expected PendingApproval");
        };
        assert_eq!(pending.calls.len(), 2);
        assert!(pending.calls.iter().any(|c| c.id == "risky" && c.requires_confirmation));
        assert!(pending.calls.iter().any(|c| c.id == "safe" && !c.requires_confirmation));
        assert!(tool_events(&seen).is_empty(), "nothing in the round may execute");
        assert!(!root.join("new.adoc").exists(), "the gated write must not have happened");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_cancelled_turn_stops_before_executing_the_round_it_was_in() {
        let root = fixture_repo("cancel");
        let provider = ScriptedProvider::new(vec![round(
            "",
            vec![tool_call("c1", "readFile", r#"{"path":"intro.adoc"}"#)],
        )]);
        let (events, seen) = collector();
        // Set before the first checkpoint — the turn resolves as cancelled
        // without ever calling the model.
        let cancel = ChatCancelFlag::new(true);

        let outcome = run(&provider, &root, &events, &cancel).unwrap();

        assert!(matches!(outcome, ChatStreamOutcome::Cancelled(_)));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert!(tool_events(&seen).is_empty());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_model_that_never_stops_asking_for_tools_hits_the_limit_instead_of_looping() {
        let root = fixture_repo("budget");
        // Every round asks for another tool, forever. Without the
        // iteration/budget ceiling this test would hang rather than fail.
        let provider = ScriptedProvider::repeating(round(
            "",
            vec![tool_call("c", "readFile", r#"{"path":"intro.adoc"}"#)],
        ));
        let (events, _seen) = collector();
        let cancel = ChatCancelFlag::new(false);

        let err = run(&provider, &root, &events, &cancel).unwrap_err();

        assert!(err.contains("инструментам"), "user-facing limit message, got: {err}");
        assert!(
            provider.calls.load(Ordering::SeqCst) <= MAX_TOOL_ITERATIONS,
            "must stop at the ceiling, not past it"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_call_failing_path_preflight_becomes_a_tool_error_with_no_approval_card() {
        let root = fixture_repo("preflight");
        let provider = ScriptedProvider::new(vec![
            // Outside the scope root — an impossible write must fail as a
            // tool error, not surface a confirmation card for it.
            round("", vec![tool_call("c1", "writeFile", r#"{"path":"../escape.adoc","content":"x"}"#)]),
            round("Не вышло.", vec![]),
        ]);
        let (events, seen) = collector();
        let cancel = ChatCancelFlag::new(false);

        let outcome = run(&provider, &root, &events, &cancel).unwrap();

        assert!(matches!(outcome, ChatStreamOutcome::Done(_)), "no approval card");
        let events = seen.lock().unwrap();
        let errored = events.iter().any(|e| matches!(e, ChatEvent::ToolResult(r) if r.error.is_some()));
        assert!(errored, "the rejected call should have reported a tool error");

        std::fs::remove_dir_all(&root).ok();
    }

    // --- `requestArtifact` ---

    fn artifact_call() -> LlmToolCall {
        tool_call(
            "a1",
            "requestArtifact",
            r#"{"kind":"httpRequest","title":"Создание документа","purpose":"Нужны входные параметры"}"#,
        )
    }

    #[test]
    fn a_request_artifact_round_pauses_without_executing_it() {
        let root = fixture_repo("artifact-pause");
        let provider = ScriptedProvider::new(vec![round("", vec![artifact_call()])]);
        let (events, seen) = collector();
        let cancel = ChatCancelFlag::new(false);

        let outcome = run(&provider, &root, &events, &cancel).unwrap();

        let ChatStreamOutcome::PendingApproval(pending) = outcome else {
            panic!("expected PendingApproval");
        };
        assert_eq!(pending.calls.len(), 1);
        assert_eq!(pending.calls[0].name, "requestArtifact");
        assert!(pending.calls[0].requires_confirmation);
        assert!(tool_events(&seen).is_empty(), "nothing ran before the user decided");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn deferring_an_artifact_lets_the_turn_continue_without_it() {
        // «Заполню позже» is not a refusal: the model is told the artifact
        // was skipped, in Russian, and carries on — same wording `askUser`
        // uses, and deliberately not the harsher "Отклонено пользователем".
        let root = fixture_repo("artifact-defer");
        let provider = ScriptedProvider::new(vec![round("Хорошо, продолжу без него.", vec![])]);
        let (events, _seen) = collector();
        let cancel = ChatCancelFlag::new(false);

        let outcome = run_resumed(
            &provider,
            &root,
            &events,
            &cancel,
            vec![artifact_call()],
            vec![ToolCallDecision {
                id: "a1".to_string(),
                approved: false,
                answer: None,
                artifact_id: None,
            }],
        )
        .unwrap();

        let ChatStreamOutcome::Done(done) = outcome else {
            panic!("expected Done");
        };
        assert_eq!(done.result.text, "Хорошо, продолжу без него.");
        assert_eq!(provider.tool_reply("a1"), "Пропущено пользователем");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unreadable_artifact_is_a_tool_error_the_model_can_react_to() {
        // The decision carries only an id, so the record is loaded here and
        // can fail (deleted between filling it in and resuming). That must
        // reach the model as a recoverable tool error, never hard-fail the
        // turn.
        let root = fixture_repo("artifact-missing");
        let provider = ScriptedProvider::new(vec![round("Не смог прочитать артефакт.", vec![])]);
        let (events, _seen) = collector();
        let cancel = ChatCancelFlag::new(false);

        let outcome = run_resumed(
            &provider,
            &root,
            &events,
            &cancel,
            vec![artifact_call()],
            vec![ToolCallDecision {
                id: "a1".to_string(),
                approved: true,
                answer: None,
                artifact_id: Some("does-not-exist".to_string()),
            }],
        )
        .unwrap();

        assert!(matches!(outcome, ChatStreamOutcome::Done(_)), "not a hard failure");
        let reply = provider.tool_reply("a1");
        assert!(reply.starts_with("Ошибка: "), "unexpected tool reply: {reply}");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reported_usage_is_recorded_and_announced() {
        let root = fixture_repo("usage");
        let mut only = round("Готово.", vec![]);
        only.usage = Some(ChatUsage { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 });
        let provider = ScriptedProvider::new(vec![only]);
        let (events, seen) = collector();
        let cancel = ChatCancelFlag::new(false);

        run(&provider, &root, &events, &cancel).unwrap();

        assert!(
            seen.lock().unwrap().iter().any(|e| matches!(e, ChatEvent::RateLimitChanged)),
            "the status-bar chip is driven by this event"
        );
        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, ChatEvent::ContextUsage(u) if u.total_tokens == 15)),
            "the chat panel's context ring is driven by this event"
        );

        std::fs::remove_dir_all(&root).ok();
    }

}
