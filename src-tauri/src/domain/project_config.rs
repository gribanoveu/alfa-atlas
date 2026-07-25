use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stable per-repo config stored at `{repoRoot}/.docflow/project.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    /// Path to documentation root, relative to the repository root (`"."` = repo root).
    pub docs_root: String,
}

impl ProjectConfig {
    pub fn new(docs_root_relative: impl Into<String>) -> Self {
        Self {
            docs_root: docs_root_relative.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("path is not a directory: {0}")]
    NotADirectory(String),
    #[error("failed to resolve path: {0}")]
    Canonicalize(#[source] std::io::Error),
    #[error("failed to create directory: {0}")]
    CreateDir(#[source] std::io::Error),
    #[error("failed to read file: {0}")]
    Read(#[source] std::io::Error),
    #[error("failed to write file: {0}")]
    Write(#[source] std::io::Error),
    #[error("failed to parse project config: {0}")]
    Parse(#[source] serde_json::Error),
    #[error("failed to serialize project config: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("documentation root is outside the repository: {0}")]
    DocsOutsideRepo(String),
    #[error("path escapes documentation root: {0}")]
    PathEscape(String),
    #[error("unsupported file type: {0}")]
    UnsupportedFile(String),
    #[error("file not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedProject {
    pub root: String,
    pub docs_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsCandidate {
    pub path: String,
    pub relative_path: String,
    pub score: u32,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub needs_confirm: bool,
    pub root: String,
    pub docs_root: Option<String>,
    pub candidates: Vec<DocsCandidate>,
    pub suggested_docs_root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<TreeNode>>,
}
