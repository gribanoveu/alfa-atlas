use crate::domain::workspace::WorkspaceState;
use crate::services::workspace_state;

#[tauri::command]
pub fn get_workspace_state(project_root: String) -> Result<WorkspaceState, String> {
    workspace_state::load_workspace(&project_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_workspace_state(
    project_root: String,
    state: WorkspaceState,
) -> Result<(), String> {
    workspace_state::save_workspace(&project_root, state).map_err(|e| e.to_string())
}
