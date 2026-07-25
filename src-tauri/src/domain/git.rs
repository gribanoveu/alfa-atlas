use serde::Serialize;
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
    #[error("{0}")]
    Message(String),
}
