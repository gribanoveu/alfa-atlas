use std::path::Path;

use crate::domain::git::{
    CheckoutOutcome, GitBlameHunk, GitBranchInfo, GitCommitSummary, GitConflictFile, GitDiffScope,
    GitError, GitFileDiff, GitFileStatus, GitProgressEvent, GitStashEntry, GitStashRestoreOutcome,
    GitStatusSnapshot, GitSyncStatus, PullMode,
};
use crate::infra::{git_credentials_store, git_repo, key_management};

pub fn status(repo_root: &str) -> Result<GitStatusSnapshot, GitError> {
    git_repo::status(Path::new(repo_root))
}

pub fn stage(repo_root: &str, paths: &[String]) -> Result<(), GitError> {
    if paths.is_empty() {
        return Ok(());
    }
    git_repo::stage_paths(Path::new(repo_root), paths)
}

pub fn unstage(repo_root: &str, paths: &[String]) -> Result<(), GitError> {
    if paths.is_empty() {
        return Ok(());
    }
    git_repo::unstage_paths(Path::new(repo_root), paths)
}

pub fn commit(repo_root: &str, message: &str) -> Result<String, GitError> {
    git_repo::commit(Path::new(repo_root), message)
}

pub fn log(repo_root: &str, limit: usize) -> Result<Vec<GitCommitSummary>, GitError> {
    let limit = if limit == 0 { 20 } else { limit.min(100) };
    git_repo::log(Path::new(repo_root), limit)
}

pub fn pull(
    repo_root: &str,
    mode: PullMode,
    on_progress: Option<&mut dyn FnMut(GitProgressEvent)>,
) -> Result<(), GitError> {
    let credentials = git_credentials_store::load()
        .map_err(|e| GitError::Message(e.to_string()))?;
    let app_private_key = key_management::get_decrypted_private_key();
    git_repo::pull(
        Path::new(repo_root),
        mode,
        &credentials,
        app_private_key.as_deref(),
        on_progress,
    )
}

pub fn conflict_file_content(repo_root: &str, path: &str) -> Result<GitConflictFile, GitError> {
    git_repo::conflict_file_content(Path::new(repo_root), path)
}

pub fn resolve_conflict(repo_root: &str, path: &str, content: &str) -> Result<(), GitError> {
    git_repo::resolve_conflict(Path::new(repo_root), path, content)
}

pub fn finish_merge(repo_root: &str) -> Result<String, GitError> {
    git_repo::finish_merge(Path::new(repo_root))
}

pub fn abort_merge(repo_root: &str) -> Result<(), GitError> {
    git_repo::abort_merge(Path::new(repo_root))
}

pub fn sync_status(repo_root: &str) -> Result<GitSyncStatus, GitError> {
    let credentials = git_credentials_store::load()
        .map_err(|e| GitError::Message(e.to_string()))?;
    let app_private_key = key_management::get_decrypted_private_key();
    git_repo::sync_status(Path::new(repo_root), &credentials, app_private_key.as_deref())
}

pub fn reset_to_remote(repo_root: &str) -> Result<(), GitError> {
    let credentials = git_credentials_store::load()
        .map_err(|e| GitError::Message(e.to_string()))?;
    let app_private_key = key_management::get_decrypted_private_key();
    git_repo::reset_to_remote(Path::new(repo_root), &credentials, app_private_key.as_deref())
}

pub fn push(
    repo_root: &str,
    on_progress: Option<&mut dyn FnMut(GitProgressEvent)>,
) -> Result<(), GitError> {
    let credentials = git_credentials_store::load()
        .map_err(|e| GitError::Message(e.to_string()))?;
    let app_private_key = key_management::get_decrypted_private_key();
    git_repo::push(
        Path::new(repo_root),
        &credentials,
        app_private_key.as_deref(),
        on_progress,
    )
}

pub fn file_diff(
    repo_root: &str,
    path: &str,
    scope: GitDiffScope,
) -> Result<GitFileDiff, GitError> {
    git_repo::file_diff(Path::new(repo_root), path, scope)
}

pub fn commit_files(repo_root: &str, commit_hash: &str) -> Result<Vec<GitFileStatus>, GitError> {
    git_repo::commit_files(Path::new(repo_root), commit_hash)
}

pub fn commit_file_diff(
    repo_root: &str,
    commit_hash: &str,
    path: &str,
) -> Result<GitFileDiff, GitError> {
    git_repo::commit_file_diff(Path::new(repo_root), commit_hash, path)
}

pub fn blame(
    repo_root: &str,
    path: &str,
    start_line: Option<u32>,
    end_line: Option<u32>,
) -> Result<Vec<GitBlameHunk>, GitError> {
    git_repo::blame(Path::new(repo_root), path, start_line, end_line)
}

pub fn discard_file_changes(repo_root: &str, path: &str) -> Result<(), GitError> {
    git_repo::discard_file_changes(Path::new(repo_root), path)
}

pub fn apply_diff_content(
    repo_root: &str,
    path: &str,
    scope: GitDiffScope,
    content: &str,
) -> Result<(), GitError> {
    git_repo::apply_diff_content(Path::new(repo_root), path, scope, content)
}

pub fn list_branches(repo_root: &str) -> Result<Vec<GitBranchInfo>, GitError> {
    git_repo::list_branches(Path::new(repo_root))
}

pub fn fetch_branches(
    repo_root: &str,
    on_progress: Option<&mut dyn FnMut(GitProgressEvent)>,
) -> Result<(), GitError> {
    let credentials = git_credentials_store::load()
        .map_err(|e| GitError::Message(e.to_string()))?;
    let app_private_key = key_management::get_decrypted_private_key();
    git_repo::fetch_branches(
        Path::new(repo_root),
        &credentials,
        app_private_key.as_deref(),
        on_progress,
    )
}

pub fn create_branch(
    repo_root: &str,
    name: &str,
    discard_changes: bool,
) -> Result<(), GitError> {
    git_repo::create_branch(Path::new(repo_root), name, discard_changes)
}

pub fn checkout_branch(
    repo_root: &str,
    name: &str,
    discard_changes: bool,
) -> Result<CheckoutOutcome, GitError> {
    git_repo::checkout_branch(Path::new(repo_root), name, discard_changes)
}

pub fn delete_branch(repo_root: &str, name: &str) -> Result<(), GitError> {
    git_repo::delete_branch(Path::new(repo_root), name)
}

pub fn checkout_remote_branch(
    repo_root: &str,
    name: &str,
    discard_changes: bool,
) -> Result<CheckoutOutcome, GitError> {
    git_repo::checkout_remote_branch(Path::new(repo_root), name, discard_changes)
}

pub fn stash_list(repo_root: &str) -> Result<Vec<GitStashEntry>, GitError> {
    git_repo::list_stash_shelf(Path::new(repo_root))
}

pub fn stash_apply(repo_root: &str, stash_id: &str) -> Result<GitStashRestoreOutcome, GitError> {
    git_repo::apply_stash_entry(Path::new(repo_root), stash_id)
}

pub fn stash_drop(repo_root: &str, stash_id: &str) -> Result<(), GitError> {
    git_repo::drop_stash_entry(Path::new(repo_root), stash_id)
}
