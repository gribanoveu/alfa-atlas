use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::ai_access::{default_allowed_tools, AiAccessMode, ToolName};
use super::project_config::ProjectError;
use super::workspace_index::WorkspaceIndexError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListFilesArgs {
    /// Subdirectory relative to the scope root, or `None`/`"."` for the root itself.
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFileArgs {
    /// File path relative to the scope root.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolFileEntry {
    /// Path relative to the scope root, `/`-separated.
    pub path: String,
    pub is_dir: bool,
}

/// One call into the tool executor. This — not the individual `ToolName`
/// variants — is the harness-facing unit: `services::ai_tools::execute_tool`
/// takes exactly one of these and returns exactly one `ToolResult`, so a
/// future LLM tool-calling loop has a single typed request/response shape
/// to serialize, regardless of which tool it names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tool", content = "args", rename_all = "camelCase")]
pub enum ToolCall {
    ReadFile(ReadFileArgs),
    ListFiles(ListFilesArgs),
}

impl ToolCall {
    pub fn name(&self) -> ToolName {
        match self {
            ToolCall::ReadFile(_) => ToolName::ReadFile,
            ToolCall::ListFiles(_) => ToolName::ListFiles,
        }
    }
}

/// Result of a `ToolCall`. Variants are named after the shape of the
/// payload (a file's content vs. a listing), not after the tool that
/// produced it — mirrors `ToolCall` as the other half of the same
/// serialized boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "tool", content = "result", rename_all = "camelCase")]
pub enum ToolResult {
    File(String),
    FileList(Vec<ToolFileEntry>),
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("tool not allowed in this access mode: {0:?}")]
    NotAllowed(ToolName),
    #[error("path escapes tool root: {0}")]
    PathEscape(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("not a file: {0}")]
    NotAFile(String),
    #[error("io error: {0}")]
    Io(#[source] std::io::Error),
}

impl From<ProjectError> for ToolError {
    fn from(err: ProjectError) -> Self {
        match err {
            ProjectError::PathEscape(p) | ProjectError::DocsOutsideRepo(p) => {
                ToolError::PathEscape(p)
            }
            ProjectError::NotFound(p) | ProjectError::NotADirectory(p) => ToolError::NotFound(p),
            ProjectError::Canonicalize(e) | ProjectError::Read(e) => ToolError::Io(e),
            other => ToolError::Io(std::io::Error::other(other.to_string())),
        }
    }
}

impl From<WorkspaceIndexError> for ToolError {
    fn from(err: WorkspaceIndexError) -> Self {
        match err {
            WorkspaceIndexError::Io(e) => ToolError::Io(e),
            WorkspaceIndexError::PathEscape(p) => ToolError::PathEscape(p),
            other => ToolError::Io(std::io::Error::other(other.to_string())),
        }
    }
}

/// The filesystem boundary *and* tool allowlist a `ToolCall` is executed
/// against, resolved once per harness session. Both axes are independent:
/// `mode` picks the root (`docsRoot` vs `repoRoot`), `allowed_tools` picks
/// which `ToolName`s are reachable at all — a project can be `FullRepo` but
/// still have `ReadFile` disabled, for instance.
#[derive(Debug, Clone)]
pub struct ToolScope {
    pub mode: AiAccessMode,
    pub root: PathBuf,
    allowed_tools: HashSet<ToolName>,
}

impl ToolScope {
    /// `root` is `docs_root` in `DocsOnly` mode, `repo_root` in `FullRepo`
    /// mode — the executor decides this itself rather than trusting a
    /// caller-supplied root, so a harness cannot widen its own access by
    /// passing the wrong path in. `allowed_tools` should already be
    /// resolved (e.g. via `services::ai_tools::scope_for_config`, which
    /// falls back to `default_allowed_tools` when a project hasn't
    /// customized it).
    pub fn new(
        repo_root: &Path,
        docs_root: &Path,
        mode: AiAccessMode,
        allowed_tools: HashSet<ToolName>,
    ) -> Self {
        let root = match mode {
            AiAccessMode::DocsOnly => docs_root,
            AiAccessMode::FullRepo => repo_root,
        };
        Self {
            mode,
            root: root.to_path_buf(),
            allowed_tools,
        }
    }

    /// Convenience for the common case — no persisted allowlist
    /// customization yet, use `default_allowed_tools` for `mode`.
    pub fn for_project(repo_root: &Path, docs_root: &Path, mode: AiAccessMode) -> Self {
        Self::new(repo_root, docs_root, mode, default_allowed_tools(mode))
    }

    pub fn allows(&self, tool: ToolName) -> bool {
        self.allowed_tools.contains(&tool)
    }
}
