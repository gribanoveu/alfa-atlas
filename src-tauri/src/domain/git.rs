use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileStatus {
    pub path: String,
    /// Single-letter status: M, A, D, R, ?
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusSnapshot {
    pub staged: Vec<GitFileStatus>,
    pub unstaged: Vec<GitFileStatus>,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitSummary {
    pub hash: String,
    pub message: String,
    pub author: String,
    /// Unix timestamp (seconds).
    pub time: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PullMode {
    Merge,
    Rebase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GitDiffScope {
    Staged,
    Unstaged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileDiff {
    pub original: String,
    pub modified: String,
    pub original_label: String,
    pub modified_label: String,
    pub is_binary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchInfo {
    pub name: String,
    pub is_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSyncStatus {
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("path is not a git repository: {0}")]
    NotARepository(String),
    #[error("failed to open repository: {0}")]
    Open(#[source] git2::Error),
    #[error("git operation failed: {0}")]
    Operation(#[source] git2::Error),
    #[error("commit message is empty")]
    EmptyMessage,
    #[error("nothing staged to commit")]
    NothingStaged,
    #[error(
        "git user.name / user.email are not configured; set them with git config user.name / user.email"
    )]
    MissingIdentity,
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("current branch has no upstream remote tracking branch")]
    NoUpstream,
    #[error("merge conflict; resolve conflicts manually and try again")]
    MergeConflict,
    #[error("rebase conflict; resolve conflicts manually and try again")]
    RebaseConflict,
    #[error("branch not found: {0}")]
    BranchNotFound(String),
    #[error("branch already exists: {0}")]
    BranchAlreadyExists(String),
    #[error("commit or discard tracked changes before switching branches")]
    CheckoutBlocked,
    #[error("{0}")]
    Message(String),
}
