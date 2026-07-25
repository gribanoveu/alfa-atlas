use crate::services::project_settings;

#[tauri::command]
pub fn get_project_root() -> Result<Option<String>, String> {
    project_settings::load_project_root().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_project_root(path: String) -> Result<String, String> {
    project_settings::set_project_root(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_project_root() -> Result<(), String> {
    project_settings::clear_project_root().map_err(|e| e.to_string())
}
