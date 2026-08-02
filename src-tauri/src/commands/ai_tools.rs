use crate::domain::ai_tools::{ToolCall, ToolResult};
use crate::services::ai_tools;

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
