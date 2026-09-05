//! The `calendar:*` channel names the frontend subscribes to, and the adapter
//! that turns `domain::calendar::CalendarNotice` into them.
//!
//! Same split as `commands::workspace_events`: the background loops report
//! through a sink and know nothing about Tauri; this is the one place those
//! reports become events.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::domain::calendar::{CalendarEvent, CalendarNotice, CalendarNoticeSink};

/// A sync refreshed the cache — carries the whole window, so the panel can
/// replace what it holds instead of merging.
pub const UPDATED_EVENT: &str = "calendar:updated";

/// A meeting is about to start. One per meeting per run.
pub const REMINDER_EVENT: &str = "calendar:reminder";

/// Mirrors `CalendarReminder` in `src/lib/calendar.ts`.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReminderPayload {
    event: CalendarEvent,
    /// Minutes until the start, as the notification should word it.
    minutes: u32,
}

pub fn calendar_notice_sink(app: &AppHandle) -> CalendarNoticeSink {
    let app = app.clone();
    Arc::new(move |notice: CalendarNotice| {
        let _ = match notice {
            CalendarNotice::Updated(events) => app.emit(UPDATED_EVENT, &events),
            CalendarNotice::Reminder { event, minutes } => {
                app.emit(REMINDER_EVENT, ReminderPayload { event, minutes })
            }
        };
    })
}
