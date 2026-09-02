//! Tauri commands for the Jira integration: settings CRUD, write-only token
//! storage, and the single network call the feature makes — "who is this
//! token?", which doubles as the connection check.
//!
//! Same credential contract as `commands::llm`: the token goes in and never
//! comes back out, only a boolean `jira_has_token` status.

use crate::domain::jira::{JiraSettings, JiraSettingsView, JiraUser};
use crate::infra::jira_credentials_store;
use crate::services::jira_config;

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
