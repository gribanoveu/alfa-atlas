use crate::domain::layout::PanelLayout;
use crate::services::project_layout;

#[tauri::command]
pub fn get_project_layout(project_root: String) -> Result<PanelLayout, String> {
    project_layout::load_layout(&project_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_project_layout(project_root: String, layout: PanelLayout) -> Result<(), String> {
    project_layout::save_layout(&project_root, layout).map_err(|e| e.to_string())
}
