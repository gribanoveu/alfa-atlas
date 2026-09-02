//! Tauri commands for the Jira integration: settings CRUD, write-only token
//! storage, and the single network call the feature makes — "who is this
//! token?", which doubles as the connection check.
//!
//! Same credential contract as `commands::llm`: the token goes in and never
//! comes back out, only a boolean `jira_has_token` status.

use crate::domain::jira::{
    JiraIssueType, JiraLinkOutcome, JiraProject, JiraSettings, JiraSettingsView, JiraUser,
    JiraWebLink,
};
use crate::infra::jira_credentials_store;
use crate::services::{jira_config, jira_publish};

/// Returns the user's own settings plus what the build manifest would fall
/// back to — the form edits the former and labels the latter, rather than
/// showing merged values it would then re-save as overrides.
#[tauri::command]
pub fn jira_get_settings() -> Result<JiraSettingsView, String> {
    jira_config::load_jira_settings_view().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn jira_set_settings(settings: JiraSettings) -> Result<(), String> {
    jira_config::save_jira_settings(settings).map_err(|e| e.to_string())
}

/// Write-only, mirrors `commands::llm::llm_set_api_key`.
#[tauri::command]
pub fn jira_set_token(token: String) -> Result<(), String> {
    jira_credentials_store::save_token(&token)
}

#[tauri::command]
pub fn jira_has_token() -> bool {
    jira_credentials_store::has_token()
}

#[tauri::command]
pub fn jira_delete_token() -> Result<(), String> {
    jira_credentials_store::delete_token()
}

/// End-to-end verification of the whole client (settings → token → TLS →
/// HTTP → parsing) that also *is* the panel's content: the right-dock panel
/// shows whoever comes back, and an `Err` here is exactly the "connection
/// does not work" state it renders.
#[tauri::command]
pub async fn jira_current_user() -> Result<JiraUser, String> {
    tauri::async_runtime::spawn_blocking(|| jira_config::current_user().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/// Attaches links to an existing issue as Jira Web Links — what the issue's
/// own Links panel shows, as opposed to the same URLs sitting in the
/// description text. Real tickets carry both.
///
/// Deliberately not offered to the assistant as a tool: this writes to a
/// tracker everyone on the team reads, so it stays an action the user takes
/// deliberately. Idempotent per link (see `JiraWebLink::to_payload`), so a
/// retry after a partial failure does not duplicate what already landed.
#[tauri::command]
pub async fn jira_attach_web_links(
    issue_key: String,
    links: Vec<JiraWebLink>,
) -> Result<Vec<JiraLinkOutcome>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        jira_config::attach_web_links(&issue_key, &links).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Projects for the picker. `recent_only` is the default: the instance has
/// thousands of projects, and the ten the user last worked in are what they
/// almost always want. The full list is fetched once when they start
/// searching and filtered in the UI.
#[tauri::command]
pub async fn jira_list_projects(recent_only: bool) -> Result<Vec<JiraProject>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        jira_config::list_projects(recent_only).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The issue types the given project accepts, sub-tasks excluded.
#[tauri::command]
pub async fn jira_list_issue_types(project_key: String) -> Result<Vec<JiraIssueType>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        jira_config::list_issue_types(&project_key).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Publishes a ticket artifact as a real Jira issue, records the issue key
/// on the artifact, and attaches its links.
///
/// Deliberately not a tool the assistant can call: this creates something
/// in a tracker the whole team reads and there is no undo, so it stays an
/// action the user takes knowingly. Refuses an artifact that already has a
/// key rather than creating a duplicate.
#[tauri::command]
pub async fn jira_publish_ticket(
    artifact_id: String,
) -> Result<jira_publish::PublishOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        jira_publish::publish(&artifact_id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The page for an already-published issue. `None` when Jira has no address
/// configured — the same rule as publishing, kept on this side of the wire
/// so the link is never rebuilt by hand in the UI.
#[tauri::command]
pub fn jira_issue_url(issue_key: String) -> Option<String> {
    jira_publish::issue_url(&issue_key)
}
