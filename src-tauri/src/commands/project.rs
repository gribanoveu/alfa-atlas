use crate::domain::project_config::{OpenedProject, ProbeResult, RecentProject, TreeNode};
use crate::services::{docs_fs, project_open};

#[tauri::command]
pub fn probe_open_path(path: String) -> Result<ProbeResult, String> {
    project_open::probe_open_path(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_project(root: String, docs_root: String) -> Result<OpenedProject, String> {
    project_open::open_project(&root, &docs_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_cached_project(root: String) -> Result<OpenedProject, String> {
    project_open::open_cached_project(&root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_project() -> Result<Option<OpenedProject>, String> {
    project_open::get_project().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_saved_repo_root() -> Result<Option<String>, String> {
    project_open::get_saved_repo_root().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_project() -> Result<(), String> {
    project_open::clear_project().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_recent_projects() -> Result<Vec<RecentProject>, String> {
    project_open::list_recent_projects().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_recent_project(root: String) -> Result<(), String> {
    project_open::remove_recent_project(&root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_git_branch(root: String) -> Result<Option<String>, String> {
    Ok(project_open::get_git_branch(&root))
}

#[tauri::command]
pub fn list_docs_tree(docs_root: String) -> Result<Vec<TreeNode>, String> {
    docs_fs::list_docs_tree(&docs_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_project_file(docs_root: String, relative_path: String) -> Result<String, String> {
    docs_fs::read_project_file(&docs_root, &relative_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn write_project_file(
    docs_root: String,
    relative_path: String,
    content: String,
) -> Result<(), String> {
    docs_fs::write_project_file(&docs_root, &relative_path, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_project_file(docs_root: String, relative_path: String) -> Result<(), String> {
    docs_fs::create_project_file(&docs_root, &relative_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_project_dir(docs_root: String, relative_path: String) -> Result<(), String> {
    docs_fs::create_project_dir(&docs_root, &relative_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_project_file(docs_root: String, relative_path: String) -> Result<(), String> {
    docs_fs::delete_project_file(&docs_root, &relative_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_project_dir(docs_root: String, relative_path: String) -> Result<(), String> {
    docs_fs::delete_project_dir(&docs_root, &relative_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_project_file(
    docs_root: String,
    from_relative: String,
    to_relative: String,
) -> Result<(), String> {
    docs_fs::rename_project_file(&docs_root, &from_relative, &to_relative)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_project_dir(
    docs_root: String,
    from_relative: String,
    to_relative: String,
) -> Result<(), String> {
    docs_fs::rename_project_dir(&docs_root, &from_relative, &to_relative)
        .map_err(|e| e.to_string())
}
