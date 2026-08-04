use crate::domain::ai_access::AiAccessMode;
use crate::domain::ai_tools::{ToolCall, ToolResult};
use crate::domain::project_config::ProjectConfig;
use crate::infra::project_store;
use crate::services::ai_tools;
use crate::services::project_open;

/// The frontend passes only `{ tool, args }` — no `docsRoot`/`repoRoot`, no
/// access mode. `services::ai_tools::current_scope()` resolves whichever
/// project is currently open and its persisted allowlist/mode itself.
/// `spawn_blocking` because `ListFiles` in `FullRepo` mode walks the whole
/// repo (`infra::workspace_scanner::scan_all`), comparable in cost to
/// `check_standards`'s repo-wide walk.
#[tauri::command]
pub async fn ai_execute_tool(call: ToolCall) -> Result<ToolResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let scope = ai_tools::current_scope().map_err(|e| e.to_string())?;
        ai_tools::execute_tool(&scope, call).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Loads the currently open project's persisted `AiAccessMode` — the same
/// boundary `ai_execute_tool` and `embedding_sync` both read via
/// `ProjectConfig`. Missing `project.json` → the safe `DocsOnly` default,
/// same fallback `ProjectConfig::new` uses everywhere else.
#[tauri::command]
pub fn ai_get_access_mode() -> Result<AiAccessMode, String> {
    let project = project_open::get_project()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no project is open".to_string())?;
    let config = project_store::load(&project.root)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| ProjectConfig::new(project.docs_root.clone()));
    Ok(config.ai_access_mode)
}

/// Persists a new `AiAccessMode` for the currently open project —
/// preserves any existing `ai_allowed_tools` override rather than
/// resetting it, since only the root boundary is what this changes.
#[tauri::command]
pub fn ai_set_access_mode(mode: AiAccessMode) -> Result<(), String> {
    let project = project_open::get_project()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no project is open".to_string())?;
    let mut config = project_store::load(&project.root)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| ProjectConfig::new(project.docs_root.clone()));
    config.ai_access_mode = mode;
    project_store::save(&project.root, &config).map_err(|e| e.to_string())
}
