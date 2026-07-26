use crate::domain::git::{GitCommitSummary, GitDiffScope, GitFileDiff, GitStatusSnapshot, PullMode};
use crate::services::git_ops;

#[tauri::command]
pub fn git_status(repo_root: String) -> Result<GitStatusSnapshot, String> {
    git_ops::status(&repo_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_stage(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    git_ops::stage(&repo_root, &paths).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_unstage(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    git_ops::unstage(&repo_root, &paths).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_commit(repo_root: String, message: String) -> Result<String, String> {
    git_ops::commit(&repo_root, &message).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_log(repo_root: String, limit: Option<usize>) -> Result<Vec<GitCommitSummary>, String> {
    git_ops::log(&repo_root, limit.unwrap_or(20)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn git_pull(repo_root: String, mode: PullMode) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_ops::pull(&repo_root, mode).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_reset_to_remote(repo_root: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_ops::reset_to_remote(&repo_root).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_push(repo_root: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_ops::push(&repo_root).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn git_file_diff(
    repo_root: String,
    path: String,
    scope: GitDiffScope,
) -> Result<GitFileDiff, String> {
    git_ops::file_diff(&repo_root, &path, scope).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_discard_file_changes(repo_root: String, path: String) -> Result<(), String> {
    git_ops::discard_file_changes(&repo_root, &path).map_err(|e| e.to_string())
}
