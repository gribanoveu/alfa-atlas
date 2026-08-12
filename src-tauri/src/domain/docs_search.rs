//! Types for user-facing documentation text search (`docs_search` IPC).
//! Shares `GrepArgs` / `GrepMatch` with the AI `grep` tool — same match
//! shape, separate error enum so the UI path never depends on `ToolError`.

use super::ai_tools::GrepMatch;
use super::project_config::ProjectError;
use super::workspace_index::WorkspaceIndexError;
use serde::Serialize;
use thiserror::Error;

/// Settled docs/grep search — same payload the AI tool returns inside
/// `ToolResult::GrepResults`, exposed as a standalone DTO for the
/// user-facing IPC command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepResultsPayload {
    pub matches: Vec<GrepMatch>,
    pub truncated: bool,
}

#[derive(Debug, Error)]
pub enum DocsSearchError {
    #[error("invalid pattern: {0}")]
    InvalidPattern(String),
    #[error("path escapes search root: {0}")]
    PathEscape(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(#[source] std::io::Error),
}

impl From<ProjectError> for DocsSearchError {
    fn from(err: ProjectError) -> Self {
        match err {
            ProjectError::PathEscape(p) | ProjectError::DocsOutsideRepo(p) => {
                DocsSearchError::PathEscape(p)
            }
            ProjectError::NotFound(p) | ProjectError::NotADirectory(p) => {
                DocsSearchError::NotFound(p)
            }
            ProjectError::Canonicalize(e) | ProjectError::Read(e) => DocsSearchError::Io(e),
            other => DocsSearchError::Io(std::io::Error::other(other.to_string())),
        }
    }
}

impl From<WorkspaceIndexError> for DocsSearchError {
    fn from(err: WorkspaceIndexError) -> Self {
        match err {
            WorkspaceIndexError::Io(e) => DocsSearchError::Io(e),
            WorkspaceIndexError::PathEscape(p) => DocsSearchError::PathEscape(p),
            other => DocsSearchError::Io(std::io::Error::other(other.to_string())),
        }
    }
}
