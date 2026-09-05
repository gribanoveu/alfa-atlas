//! The `llm:*` event names the frontend subscribes to, and the adapter that
//! turns `domain::llm::ChatTurnEvent` into them.
//!
//! Their own module rather than `commands::llm`'s private business: both
//! `commands::llm` and `commands::memory_pipeline` report through the same
//! sink, and having one import the other would just be a cross-import
//! between two boundary modules wearing a different hat.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::domain::llm::{ChatEventPayload, ChatEventSink, ChatTurnEvent};

/// Everything one chat turn reports, on one ordered channel.
///
/// There used to be ten separate topics here (`llm:chat-stream-delta`,
/// `llm:tool-call`, `llm:round-text`, …), each a global Tauri channel the
/// frontend re-assembled into a transcript by scanning backwards for "the
/// block this probably belongs to". Ordering between them was never stated,
/// only hoped for. This one carries `seq`/`round`/`targetId`, so the reducer
/// applies an event to a named block and can buffer or drop anything that
/// arrives late, twice, or out of order — see `chatTurnReducer.ts`.
pub const CHAT_TURN_EVENT: &str = "llm:turn-event";

/// Fires after completion tokens are recorded into the rate-limit store,
/// and after LLM settings are saved (the tracking toggle lives there) —
/// the status-bar chip refreshes without waiting for its poll interval.
///
/// Kept off `CHAT_TURN_EVENT` on purpose: its listener
/// (`useLlmRateLimit`) is not turn-scoped and must also hear the one-shot
/// calls below, which belong to no turn at all.
pub const RATE_LIMIT_CHANGED_EVENT: &str = "llm:rate-limit-changed";

/// The `turn_id` for a sink that does not belong to a chat turn at all —
/// history compaction (`llm_chat_once`) and the memory pipeline. Both only
/// ever report `RateLimitChanged`; the constant exists so those call sites
/// read as deliberate rather than as a missing id.
pub const NO_CHAT_TURN: &str = "none";

/// One turn event on the wire: the domain event plus the turn it belongs to.
///
/// `#[serde(flatten)]`, so `type`/`payload`/`seq`/`round`/`targetId` stay at
/// the top level and `LlmTurnEvent` in `src/lib/llm.ts` is a flat union.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct WithTurn {
    turn_id: String,
    #[serde(flatten)]
    event: ChatTurnEvent,
}

/// Turns `services::llm_chat`'s framework-free reports into real Tauri
/// events. This is the only place either event above is emitted — the chat
/// loop itself has no `AppHandle` and no idea a UI is listening.
///
/// `turn_id` identifies the turn this sink belongs to, and is stamped onto
/// every payload it emits. The channel is global (one per name, not per
/// window or per request), so without the id two overlapping turns would
/// interleave their deltas character by character into one message, which is
/// exactly as readable as it sounds. It stays a transport concern:
/// `domain::llm::ChatTurnEvent` never learns about it.
pub fn chat_event_sink(app: &AppHandle, turn_id: String) -> ChatEventSink {
    let app = app.clone();
    Arc::new(move |event: ChatTurnEvent| {
        // Not turn-scoped, and the one-shot sinks below emit nothing else.
        if matches!(event.event, ChatEventPayload::RateLimitChanged) {
            let _ = app.emit(RATE_LIMIT_CHANGED_EVENT, ());
        }
        if turn_id == NO_CHAT_TURN {
            return;
        }
        let _ = app.emit(
            CHAT_TURN_EVENT,
            WithTurn {
                turn_id: turn_id.clone(),
                event,
            },
        );
    })
}
