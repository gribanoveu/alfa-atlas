//! The channel names the frontend subscribes to for workspace-index
//! activity, and the adapter that turns `domain::workspace_index::
//! WorkspaceIndexEvent` into them.
//!
//! Same split as `commands::chat_events`: the service reports through a
//! sink and knows nothing about Tauri; this is the one place those reports
//! become events.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::domain::workspace_index::{WorkspaceIndexEvent, WorkspaceIndexEventSink};

/// Index lifecycle: build started/progress/finished, a document updated, its
/// diagnostics recomputed. Carries `IndexEvent`'s tagged representation.
pub const EVENT_CHANNEL: &str = "workspace-index://event";

/// A request for the frontend to parse an AsciiDoc document and send the
/// facts back via `commands::asciidoc::submit_asciidoc_facts` — the backend
/// has no AsciiDoc parser of its own.
pub const ASCIIDOC_PARSE_REQUESTED_CHANNEL: &str = "asciidoc:parse-requested";

pub fn workspace_index_event_sink(app: &AppHandle) -> WorkspaceIndexEventSink {
    let app = app.clone();
    Arc::new(move |event: WorkspaceIndexEvent| {
        let _ = match event {
            WorkspaceIndexEvent::Index(e) => app.emit(EVENT_CHANNEL, &e),
            WorkspaceIndexEvent::AsciiDocParseRequested(p) => {
                app.emit(ASCIIDOC_PARSE_REQUESTED_CHANNEL, &p)
            }
        };
    })
}
