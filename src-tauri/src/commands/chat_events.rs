//! The `llm:*` event names the frontend subscribes to, and the adapter that
//! turns `domain::llm::ChatEvent` into them.
//!
//! Their own module rather than `commands::llm`'s private business: both
//! `commands::llm` and `commands::memory_pipeline` report through the same
//! sink, and having one import the other would just be a cross-import
//! between two boundary modules wearing a different hat.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::domain::llm::{ChatEvent, ChatEventSink};

/// Fires once per non-empty text chunk while `llm_chat_stream`'s promise is
/// still in flight. Carries a `turnId` like every other event here — see
/// `chat_event_sink`.
pub const CHAT_STREAM_DELTA_EVENT: &str = "llm:chat-stream-delta";

/// Same shape/lifecycle as `CHAT_STREAM_DELTA_EVENT`, but for a
/// reasoning-capable model's "thinking" text (`reasoning_content` on the
/// wire, see `infra::llm_providers::openai_compatible::StreamDelta`) —
/// fires while the model is still reasoning, ahead of any
/// `CHAT_STREAM_DELTA_EVENT` for that round. Never fires at all for a
/// provider/model that doesn't send `reasoning_content`.
pub const CHAT_STREAM_REASONING_EVENT: &str = "llm:chat-stream-reasoning-delta";

/// Fires when queued user guidance is added to the history of a fresh round.
pub const STEERING_APPLIED_EVENT: &str = "llm:steering-applied";

/// Fires immediately before each model round of a turn — including the
/// first. The frontend closes the previous round's open text/reasoning
/// blocks on it, so two rounds' prose can never be appended into one block
/// (see `ChatEvent::RoundStarted`).
pub const ROUND_STARTED_EVENT: &str = "llm:round-started";

/// Fires once each model round has finished streaming, carrying that
/// round's full text. The frontend overwrites the round's text block with
/// it, so a dropped `CHAT_STREAM_DELTA_EVENT` cannot leave the transcript
/// permanently truncated mid-word (see `ChatEvent::RoundText`).
pub const ROUND_TEXT_EVENT: &str = "llm:round-text";

/// Fires while a tool call's arguments are still arriving on the SSE
/// stream — same payload as `TOOL_CALL_EVENT`, but the JSON may be
/// incomplete. Lets the UI show the call (and, for `visualize`, the
/// diagram source being written) instead of a silent hang. Always followed
/// later by `TOOL_CALL_EVENT` with the same `id`, unless the turn is
/// cancelled first.
pub const TOOL_CALL_DELTA_EVENT: &str = "llm:tool-call-delta";

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

/// Fires after each LLM round inside one turn for which the provider
/// reported usage — carries that `ChatUsage` so the chat panel's context
/// ring can snap to the provider's own number instead of coasting on its
/// character estimate until the whole turn finishes. Never fires for a
/// provider that doesn't send usage, and never for `llm_chat_once`'s
/// one-shot calls (compaction, the memory pipeline).
pub const CONTEXT_USAGE_EVENT: &str = "llm:context-usage";

/// Fires after completion tokens are recorded into the rate-limit store,
/// and after LLM settings are saved (the tracking toggle lives there) —
/// the status-bar chip refreshes without waiting for its poll interval.
pub const RATE_LIMIT_CHANGED_EVENT: &str = "llm:rate-limit-changed";

/// Stamps the turn a payload belongs to onto the payload itself.
///
/// `#[serde(flatten)]`, so the wire shape stays exactly what it was plus one
/// `turnId` field — no frontend type has to be restructured, only widened.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct WithTurn<T: Clone + serde::Serialize> {
    turn_id: String,
    #[serde(flatten)]
    inner: T,
}

/// The `turn_id` for a sink that does not belong to a chat turn at all —
/// history compaction (`llm_chat_once`) and the memory pipeline. Both only
/// ever report `RateLimitChanged`, which carries no payload and is not
/// filtered by turn on the frontend; the constant exists so those call
/// sites read as deliberate rather than as a missing id.
pub const NO_CHAT_TURN: &str = "none";

/// A closure cannot be generic, so the stamping the arms below all do goes
/// through this instead.
fn with_turn<T: Clone + serde::Serialize>(turn_id: &str, inner: T) -> WithTurn<T> {
    WithTurn { turn_id: turn_id.to_string(), inner }
}

/// The payload for an event that carries nothing but its turn.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TurnOnly {
    turn_id: String,
}

/// Turns `services::llm_chat`'s framework-free reports into real Tauri
/// events. This is the only place any of the `llm:*` events above is
/// emitted — the chat loop itself has no `AppHandle` and no idea a UI is
/// listening.
///
/// `turn_id` identifies the turn this sink belongs to, and is stamped onto
/// every payload it emits. These events are global (one Tauri channel per
/// name, not per window or per request), and the frontend appends whatever
/// arrives to whichever assistant message is currently streaming — so two
/// overlapping turns used to interleave their deltas character by character
/// into one message, which is exactly as readable as it sounds. The id lets
/// a listener drop what is not its own. It stays a transport concern:
/// `domain::llm::ChatEvent` never learns about it.
pub fn chat_event_sink(app: &AppHandle, turn_id: String) -> ChatEventSink {
    let app = app.clone();
    Arc::new(move |event: ChatEvent| {
        let _ = match event {
            ChatEvent::Delta(p) => app.emit(CHAT_STREAM_DELTA_EVENT, with_turn(&turn_id, p)),
            ChatEvent::Reasoning(p) => app.emit(CHAT_STREAM_REASONING_EVENT, with_turn(&turn_id, p)),
            ChatEvent::RoundStarted => {
                app.emit(ROUND_STARTED_EVENT, TurnOnly { turn_id: turn_id.clone() })
            }
            ChatEvent::RoundText(p) => app.emit(ROUND_TEXT_EVENT, with_turn(&turn_id, p)),
            ChatEvent::SteeringApplied(p) => app.emit(STEERING_APPLIED_EVENT, with_turn(&turn_id, p)),
            ChatEvent::ToolCallDelta(p) => app.emit(TOOL_CALL_DELTA_EVENT, with_turn(&turn_id, p)),
            ChatEvent::ToolCall(p) => app.emit(TOOL_CALL_EVENT, with_turn(&turn_id, p)),
            ChatEvent::ToolResult(p) => app.emit(TOOL_RESULT_EVENT, with_turn(&turn_id, p)),
            ChatEvent::RateLimitChanged => app.emit(RATE_LIMIT_CHANGED_EVENT, ()),
            ChatEvent::ContextUsage(p) => app.emit(CONTEXT_USAGE_EVENT, with_turn(&turn_id, p)),
        };
    })
}
