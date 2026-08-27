use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, State};

use crate::services::embedding_state::FullSyncActiveSlot;
use crate::domain::git::{
    AppKeyStatus, CheckoutOutcome, GitBranchInfo, GitCommitSummary, GitConflictFile,
    GitCredentials, GitDiffScope, GitFileDiff, GitFileStatus, GitProgressEvent, GitResetMode,
    GitStashEntry, GitStashRestoreOutcome, GitStatusSnapshot, GitSyncStatus, PullMode,
};
use crate::domain::project_config::ProbeResult;
use crate::services::{git_clone, git_credentials, git_ops};

/// Rejects a branch-switching command while a full `embedding_sync` walk is
/// in flight — see `FullSyncActiveSlot`'s doc comment for why this only
/// covers the first/full sync, not incremental watcher ticks.
fn reject_if_full_sync_active(flag: &FullSyncActiveSlot) -> Result<(), String> {
    if flag.load(Ordering::Acquire) {
        return Err(
            "Идёт первичная синхронизация эмбеддингов. Дождитесь её завершения, затем переключите ветку."
                .to_string(),
        );
    }
    Ok(())
}

const GIT_PROGRESS_EVENT: &str = "git://progress";
/// libgit2's `transfer_progress` fires very frequently on large repos —
/// throttle emissions to the UI so we don't flood the event channel.
const GIT_PROGRESS_THROTTLE: Duration = Duration::from_millis(100);

/// Builds a progress callback that forwards `GitProgressEvent`s to the
/// frontend over `GIT_PROGRESS_EVENT`, throttled so only ~10/sec reach the
/// UI regardless of how often libgit2 invokes the underlying callback.
/// `Started`/`Finished` are one-off lifecycle markers rather than a
/// high-frequency tick, so they always bypass the throttle.
fn progress_emitter(app: AppHandle) -> impl FnMut(GitProgressEvent) {
    let mut last = Instant::now()
        .checked_sub(GIT_PROGRESS_THROTTLE)
        .unwrap_or_else(Instant::now);
    move |event: GitProgressEvent| {
        let now = Instant::now();
        let is_lifecycle_marker =
            matches!(event, GitProgressEvent::Started { .. } | GitProgressEvent::Finished { .. });
        if is_lifecycle_marker || now.duration_since(last) >= GIT_PROGRESS_THROTTLE {
            last = now;
            let _ = app.emit(GIT_PROGRESS_EVENT, &event);
        }
    }
}

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
pub fn git_unpushed_commits(
    repo_root: String,
    limit: Option<usize>,
) -> Result<Vec<GitCommitSummary>, String> {
    git_ops::unpushed_commits(&repo_root, limit.unwrap_or(50)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn git_incoming_commits(
    repo_root: String,
    limit: Option<usize>,
) -> Result<Vec<GitCommitSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_ops::incoming_commits(&repo_root, limit.unwrap_or(50)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn git_drop_unpushed_from(
    repo_root: String,
    commit_hash: String,
    mode: GitResetMode,
) -> Result<(), String> {
    git_ops::drop_unpushed_from(&repo_root, &commit_hash, mode).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_drop_all_unpushed(repo_root: String, mode: GitResetMode) -> Result<(), String> {
    git_ops::drop_all_unpushed(&repo_root, mode).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_move_unpushed_to_new_branch(repo_root: String, new_name: String) -> Result<(), String> {
    git_ops::move_unpushed_to_new_branch(&repo_root, &new_name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_move_unpushed_to_branch(
    repo_root: String,
    target_branch: String,
) -> Result<(), String> {
    git_ops::move_unpushed_to_branch(&repo_root, &target_branch).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn git_pull(repo_root: String, mode: PullMode, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut emit = progress_emitter(app);
        emit(GitProgressEvent::Started { op: "pull".to_string() });
        let result = git_ops::pull(&repo_root, mode, Some(&mut emit)).map_err(|e| e.to_string());
        emit(GitProgressEvent::Finished { op: "pull".to_string() });
        result
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
pub fn git_conflict_file_content(
    repo_root: String,
    path: String,
) -> Result<GitConflictFile, String> {
    git_ops::conflict_file_content(&repo_root, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_resolve_conflict(
    repo_root: String,
    path: String,
    content: String,
) -> Result<(), String> {
    git_ops::resolve_conflict(&repo_root, &path, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_finish_merge(repo_root: String) -> Result<String, String> {
    git_ops::finish_merge(&repo_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_abort_merge(repo_root: String) -> Result<(), String> {
    git_ops::abort_merge(&repo_root).map_err(|e| e.to_string())
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
pub async fn git_push(repo_root: String, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut emit = progress_emitter(app);
        emit(GitProgressEvent::Started { op: "push".to_string() });
        let result = git_ops::push(&repo_root, Some(&mut emit)).map_err(|e| e.to_string());
        emit(GitProgressEvent::Finished { op: "push".to_string() });
        result
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
pub fn git_discard_file_changes(repo_root: String, path: String) -> Result<Option<String>, String> {
    git_ops::discard_file_changes(&repo_root, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_restore_discard_backup(repo_root: String, backup_id: String) -> Result<(), String> {
    git_ops::restore_discard_backup(&repo_root, &backup_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_undo_commit(repo_root: String, commit_hash: String) -> Result<(), String> {
    git_ops::undo_commit(&repo_root, &commit_hash).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_create_branch_at_oid(repo_root: String, name: String, oid: String) -> Result<(), String> {
    git_ops::create_branch_at_oid(&repo_root, &name, &oid).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_reset_to_oid(repo_root: String, oid: String) -> Result<(), String> {
    git_ops::reset_to_oid(&repo_root, &oid).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_head_oid(repo_root: String) -> Result<String, String> {
    git_ops::head_oid(&repo_root).map_err(|e| e.to_string())
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
pub async fn git_fetch_branches(repo_root: String, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut emit = progress_emitter(app);
        emit(GitProgressEvent::Started { op: "fetch".to_string() });
        let result = git_ops::fetch_branches(&repo_root, Some(&mut emit)).map_err(|e| e.to_string());
        emit(GitProgressEvent::Finished { op: "fetch".to_string() });
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn git_create_branch(
    repo_root: String,
    name: String,
    discard_changes: bool,
    full_sync_active: State<'_, Arc<FullSyncActiveSlot>>,
) -> Result<(), String> {
    reject_if_full_sync_active(&full_sync_active)?;
    git_ops::create_branch(&repo_root, &name, discard_changes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_checkout_branch(
    repo_root: String,
    name: String,
    discard_changes: bool,
    full_sync_active: State<'_, Arc<FullSyncActiveSlot>>,
) -> Result<CheckoutOutcome, String> {
    reject_if_full_sync_active(&full_sync_active)?;
    git_ops::checkout_branch(&repo_root, &name, discard_changes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_delete_branch(repo_root: String, name: String) -> Result<(), String> {
    git_ops::delete_branch(&repo_root, &name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_checkout_remote_branch(
    repo_root: String,
    name: String,
    discard_changes: bool,
    full_sync_active: State<'_, Arc<FullSyncActiveSlot>>,
) -> Result<CheckoutOutcome, String> {
    reject_if_full_sync_active(&full_sync_active)?;
    git_ops::checkout_remote_branch(&repo_root, &name, discard_changes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_stash_list(repo_root: String) -> Result<Vec<GitStashEntry>, String> {
    git_ops::stash_list(&repo_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_stash_apply(
    repo_root: String,
    stash_id: String,
) -> Result<GitStashRestoreOutcome, String> {
    git_ops::stash_apply(&repo_root, &stash_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_stash_drop(repo_root: String, stash_id: String) -> Result<(), String> {
    git_ops::stash_drop(&repo_root, &stash_id).map_err(|e| e.to_string())
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
pub async fn git_clone(
    url: String,
    destination: String,
    app: AppHandle,
) -> Result<ProbeResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut emit = progress_emitter(app);
        emit(GitProgressEvent::Started { op: "clone".to_string() });
        git_clone::clone_repository(&url, &destination, Some(&mut emit))?;
        emit(GitProgressEvent::Finished { op: "clone".to_string() });

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_if_full_sync_active_errs_while_a_full_sync_is_running() {
        let flag = FullSyncActiveSlot::new(true);
        assert!(reject_if_full_sync_active(&flag).is_err());
    }

    #[test]
    fn reject_if_full_sync_active_allows_checkout_when_idle() {
        let flag = FullSyncActiveSlot::new(false);
        assert!(reject_if_full_sync_active(&flag).is_ok());
    }
}
