use crate::domain::git::{
    AppKeyStatus, GitBranchInfo, GitCommitSummary, GitCredentials, GitDiffScope, GitFileDiff,
    GitFileStatus, GitStatusSnapshot, GitSyncStatus, PullMode,
};
use crate::domain::project_config::ProbeResult;
use crate::services::{git_clone, git_credentials, git_ops};

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
pub async fn git_sync_status(repo_root: String) -> Result<GitSyncStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_ops::sync_status(&repo_root).map_err(|e| e.to_string())
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
pub fn git_commit_files(
    repo_root: String,
    commit_hash: String,
) -> Result<Vec<GitFileStatus>, String> {
    git_ops::commit_files(&repo_root, &commit_hash).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_commit_file_diff(
    repo_root: String,
    commit_hash: String,
    path: String,
) -> Result<GitFileDiff, String> {
    git_ops::commit_file_diff(&repo_root, &commit_hash, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_discard_file_changes(repo_root: String, path: String) -> Result<(), String> {
    git_ops::discard_file_changes(&repo_root, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_apply_diff_content(
    repo_root: String,
    path: String,
    scope: GitDiffScope,
    content: String,
) -> Result<(), String> {
    git_ops::apply_diff_content(&repo_root, &path, scope, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_list_branches(repo_root: String) -> Result<Vec<GitBranchInfo>, String> {
    git_ops::list_branches(&repo_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn git_fetch_branches(repo_root: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_ops::fetch_branches(&repo_root).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn git_create_branch(
    repo_root: String,
    name: String,
    discard_changes: bool,
) -> Result<(), String> {
    git_ops::create_branch(&repo_root, &name, discard_changes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_checkout_branch(
    repo_root: String,
    name: String,
    discard_changes: bool,
) -> Result<(), String> {
    git_ops::checkout_branch(&repo_root, &name, discard_changes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_checkout_remote_branch(
    repo_root: String,
    name: String,
    discard_changes: bool,
) -> Result<(), String> {
    git_ops::checkout_remote_branch(&repo_root, &name, discard_changes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_get_credentials() -> Result<GitCredentials, String> {
    git_credentials::load_credentials()
}

#[tauri::command]
pub fn git_save_credentials(credentials: GitCredentials) -> Result<(), String> {
    git_credentials::save_credentials(credentials)
}

#[tauri::command]
pub fn git_get_key_status() -> Result<AppKeyStatus, String> {
    git_credentials::get_app_key_status()
}

#[tauri::command]
pub fn git_generate_key() -> Result<AppKeyStatus, String> {
    crate::infra::key_management::generate_and_store_key_app()
}

#[tauri::command]
pub fn git_import_key(source_path: String) -> Result<AppKeyStatus, String> {
    let path = std::path::Path::new(&source_path);
    crate::infra::key_management::import_key_file(path)
}

#[tauri::command]
pub async fn git_clone(url: String, destination: String) -> Result<ProbeResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_clone::clone_repository(&url, &destination)?;

        // After cloning, probe the repo to find docs root candidates.
        // The frontend will show ConfirmOpenProjectModal for the user to pick.
        let dest_path = std::path::Path::new(&destination);
        let canonical = dest_path
            .canonicalize()
            .unwrap_or_else(|_| dest_path.to_path_buf());
        let repo_root = crate::infra::git_repo::discover_repo_root(&canonical);
        let repo_root_str = repo_root.to_string_lossy().into_owned();

        crate::services::project_open::probe_open_path(&repo_root_str)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
