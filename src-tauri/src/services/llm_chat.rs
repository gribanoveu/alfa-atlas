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
    ToolCallEvent, ToolResultEvent,
};
use crate::domain::paths;
use crate::domain::repo_index::FileId;
use crate::infra::llm_debug_log;
use crate::services::ai_tools::{self, EmbeddingDeps, ToolCallLogContext};
use crate::services::llm_rate_limit;
use crate::services::llm_session;
use crate::services::llm_session::{ChatCancelFlag, LlmProviderSlot};


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
                let cancelled = || ctx.cancel_flag.load(Ordering::SeqCst);
                let raw_result = ctx.provider.chat_stream(request, &on_delta, &on_reasoning, &cancelled);
                llm_debug_log::log_response(ctx.settings.debug_logging, ctx.provider_id, round, &raw_result);
                let result = raw_result.map_err(|e| e.to_string())?;
                if let Some(usage) = result.usage {
                    llm_rate_limit::record(ctx.provider_id, usage.completion_tokens);
                    (ctx.events)(ChatEvent::RateLimitChanged);
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
            // `askUser` is special: answers come from `ToolCallDecision::
            // answer`, not from `execute_tool` (which would reject a bare
            // call). Skip → same denial path as Approve/Deny tools.
            let decision = decisions.iter().find(|d| d.id == call.id);
            let denied = decision.map(|d| !d.approved).unwrap_or(false);
            let outcome: Result<ToolResult, String> = if denied {
                Err("denied by user".to_string())
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
                // OptMem already formats wake/note/nap as agent-facing prose —
                // wrapping it in JSON would only add noise and burn tokens.
                Ok(ToolResult::Memory { text }) => text.clone(),
                // Skip path for askUser — Russian, matching the deny message
                // for mutating tools so the model continues in-language.
                Err(e) if e == "denied by user" && call.name == "askUser" => {
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

    let setup = setup(&mut ctx)?;
    let loop_ctx = LoopCtx {
        events,
        provider: setup.provider.as_ref(),
        provider_id: &ctx.provider_id,
        model: &setup.model,
        settings: &setup.settings,
        deps: &ctx.deps,
        cancel_flag: &ctx.cancel_flag,
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
}
