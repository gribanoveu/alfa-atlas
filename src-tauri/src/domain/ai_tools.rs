use std::path::{Path, PathBuf};

use thiserror::Error;

use super::ai_access::AiAccessMode;
use super::project_config::ProjectError;
use super::workspace_index::WorkspaceIndexError;

/// Read-only operations a future AI harness may invoke. Intentionally no
/// write/edit tool yet — this pass only lays the access boundary, not the
/// agent loop that would need to apply edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolName {
    ListFiles,
    ReadFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListFilesArgs {
    /// Subdirectory relative to the scope root, or `None`/`"."` for the root itself.
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFileArgs {
    /// File path relative to the scope root.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolFileEntry {
    /// Path relative to the scope root, `/`-separated.
    pub path: String,
    pub is_dir: bool,
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

/// Which tools are exposed in a given access mode. Both current tools are
/// permitted everywhere today — only the root differs (see `ToolScope`) —
/// but the check exists from day one so a future tool that reads *around*
/// the root boundary (e.g. a git-blob/diff tool, which reads repo content
/// via libgit2 independent of `ensure_under`) has to be added to this table
/// deliberately instead of being reachable by omission.
pub fn is_tool_allowed(_mode: AiAccessMode, tool: ToolName) -> bool {
    matches!(tool, ToolName::ListFiles | ToolName::ReadFile)
}

/// The filesystem boundary a tool call is executed against, resolved once
/// per harness session from the open project's configured `AiAccessMode`.
#[derive(Debug, Clone)]
pub struct ToolScope {
    pub mode: AiAccessMode,
    pub root: PathBuf,
}

impl ToolScope {
    /// `root` is `docs_root` in `DocsOnly` mode, `repo_root` in `FullRepo`
    /// mode — the executor decides this itself rather than trusting a
    /// caller-supplied root, so a harness cannot widen its own access by
    /// passing the wrong path in.
    pub fn for_project(repo_root: &Path, docs_root: &Path, mode: AiAccessMode) -> Self {
        let root = match mode {
            AiAccessMode::DocsOnly => docs_root,
            AiAccessMode::FullRepo => repo_root,
        };
        Self {
            mode,
            root: root.to_path_buf(),
        }
    }
}
