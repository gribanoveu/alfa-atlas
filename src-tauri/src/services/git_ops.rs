use std::path::Path;

use crate::domain::git::{
    GitBranchInfo, GitCommitSummary, GitDiffScope, GitError, GitFileDiff,
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

pub fn pull(repo_root: &str, mode: PullMode) -> Result<(), GitError> {
    let credentials = git_credentials_store::load()
        .map_err(|e| GitError::Message(e.to_string()))?;
    let app_private_key = key_management::get_decrypted_private_key();
    git_repo::pull(Path::new(repo_root), mode, &credentials, app_private_key.as_deref())
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

pub fn push(repo_root: &str) -> Result<(), GitError> {
    let credentials = git_credentials_store::load()
        .map_err(|e| GitError::Message(e.to_string()))?;
    let app_private_key = key_management::get_decrypted_private_key();
    git_repo::push(Path::new(repo_root), &credentials, app_private_key.as_deref())
}

pub fn file_diff(
    repo_root: &str,
    path: &str,
    scope: GitDiffScope,
) -> Result<GitFileDiff, GitError> {
    git_repo::file_diff(Path::new(repo_root), path, scope)
}

pub fn discard_file_changes(repo_root: &str, path: &str) -> Result<(), GitError> {
    git_repo::discard_file_changes(Path::new(repo_root), path)
}

pub fn list_branches(repo_root: &str) -> Result<Vec<GitBranchInfo>, GitError> {
    git_repo::list_branches(Path::new(repo_root))
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
) -> Result<(), GitError> {
    git_repo::checkout_branch(Path::new(repo_root), name, discard_changes)
}

pub fn checkout_remote_branch(
    repo_root: &str,
    name: &str,
    discard_changes: bool,
) -> Result<(), GitError> {
    git_repo::checkout_remote_branch(Path::new(repo_root), name, discard_changes)
}
