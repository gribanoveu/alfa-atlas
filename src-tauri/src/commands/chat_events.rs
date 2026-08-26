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
pub fn chat_event_sink(app: &AppHandle) -> ChatEventSink {
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
