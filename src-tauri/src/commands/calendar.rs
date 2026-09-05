//! Tauri commands for the calendar. Settings CRUD, write-only password
//! storage, and the network calls — the latter on a blocking thread since
//! `OwaSession` is blocking (`ureq`). Errors flatten to `String` here, the one
//! place that is allowed.
//!
//! The join URL is not a command: every `CalendarEvent.join_url` was already
//! validated by `domain::calendar::safe_url` when the event was mapped, so the
//! frontend opens it directly with the opener plugin.

use std::sync::Arc;

use tauri::State;

use crate::domain::calendar::{
    CalendarEvent, CalendarSettings, CalendarSettingsView, CalendarStatus, EventDetails, RsvpAction,
};
use crate::infra::owa_credentials_store;
use crate::services::calendar_config;
use crate::services::calendar_sync::CalendarState;

#[tauri::command]
pub fn calendar_get_settings() -> Result<CalendarSettingsView, String> {
    calendar_config::load_settings_view(owa_credentials_store::has_password())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn calendar_save_settings(settings: CalendarSettings) -> Result<(), String> {
    calendar_config::save_settings(settings).map_err(|e| e.to_string())
}

/// Write-only: the password never comes back over IPC. `remember` persists it
/// encrypted; otherwise it lives only in the session for this run.
#[tauri::command]
pub async fn calendar_set_password(
    state: State<'_, Arc<CalendarState>>,
    password: String,
    remember: bool,
) -> Result<(), String> {
    let state = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        state.set_password(&password, remember).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn calendar_has_password() -> bool {
    owa_credentials_store::has_password()
}

#[tauri::command]
pub async fn calendar_forget(state: State<'_, Arc<CalendarState>>) -> Result<(), String> {
    state.forget().map_err(|e| e.to_string())
}

/// Cached events — no network, instant, what the panel shows on mount.
#[tauri::command]
pub fn calendar_get_cached(state: State<'_, Arc<CalendarState>>) -> Vec<CalendarEvent> {
    state.cached()
}

#[tauri::command]
pub fn calendar_status(state: State<'_, Arc<CalendarState>>) -> CalendarStatus {
    state.status()
}

/// Fetches now and refreshes the cache. Also the connection check: a success
/// is proof the whole chain (settings → password → TLS → NTLM → parse) worked.
#[tauri::command]
pub async fn calendar_sync(state: State<'_, Arc<CalendarState>>) -> Result<Vec<CalendarEvent>, String> {
    let state = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || state.sync().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/// Sends an RSVP (Accept/Tentative/Decline). `send` notifies the organizer
/// (`SendAndSaveCopy`); `false` updates only the user's calendar (`SaveOnly`).
#[tauri::command]
pub async fn calendar_rsvp(
    state: State<'_, Arc<CalendarState>>,
    id: String,
    change_key: String,
    action: RsvpAction,
    send: bool,
) -> Result<(), String> {
    let state = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        state.rsvp(action, &id, &change_key, send).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Attendees + body for one meeting, fetched on demand when it is expanded.
#[tauri::command]
pub async fn calendar_event_details(
    state: State<'_, Arc<CalendarState>>,
    id: String,
    change_key: Option<String>,
) -> Result<EventDetails, String> {
    let state = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        state.event_details(&id, change_key.as_deref()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
