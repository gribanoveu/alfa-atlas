use crate::domain::agent_skills::{SkillListItem, SkillMeta, SkillSource};
use crate::infra::user_skills_store;
use crate::services::agent_skills;

#[tauri::command]
pub fn skills_list() -> Result<Vec<SkillListItem>, String> {
    agent_skills::list_skills().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn skills_set_enabled(source: SkillSource, name: String, enabled: bool) -> Result<(), String> {
    agent_skills::set_skill_enabled(source, &name, enabled).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn skills_files(source: SkillSource, name: String) -> Result<Vec<String>, String> {
    agent_skills::preview_files(source, &name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn skills_read_file(source: SkillSource, name: String, path: String) -> Result<String, String> {
    agent_skills::preview_file(source, &name, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn skills_import(path: String) -> Result<SkillMeta, String> {
    user_skills_store::import_skill_dir(std::path::Path::new(&path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn skills_remove(name: String) -> Result<(), String> {
    agent_skills::remove_skill(&name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn skills_user_dir() -> Result<String, String> {
    let dir = user_skills_store::ensure_user_skills_dir().map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().into_owned())
}
