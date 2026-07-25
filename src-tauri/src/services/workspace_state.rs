use crate::domain::settings::SettingsError;
use crate::domain::workspace::WorkspaceState;
use crate::infra::workspace_store;

pub fn load_workspace(project_root: &str) -> Result<WorkspaceState, SettingsError> {
    workspace_store::load(project_root)
}

pub fn save_workspace(project_root: &str, state: WorkspaceState) -> Result<(), SettingsError> {
    workspace_store::save(project_root, &state)
}
