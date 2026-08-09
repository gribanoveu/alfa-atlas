use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::ai_access::{AiAccessMode, ToolName};

/// Stable per-repo config stored at `{repoRoot}/.atlas/project.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    /// Path to documentation root, relative to the repository root (`"."` = repo root).
    pub docs_root: String,
    /// Filesystem boundary for a future AI harness on this project.
    /// Defaults to `DocsOnly` so existing `project.json` files without this
    /// field keep the safer boundary rather than silently widening to the
    /// full repo.
    #[serde(default)]
    pub ai_access_mode: AiAccessMode,
    /// User-set tool allowlist override. `None` = use
    /// `ai_access::default_allowed_tools` for `ai_access_mode`. Once set, it
    /// is authoritative — see that function's doc comment for why a new
    /// tool isn't silently added to an already-customized list.
    #[serde(default)]
    pub ai_allowed_tools: Option<Vec<ToolName>>,
    /// Tool names the user has told the assistant to stop asking
    /// confirmation for on this project, via the approval card's "Разрешать
    /// всегда" button (see `services::ai_tools::set_tool_auto_approved`).
    /// `None`/missing (old `project.json` files) behaves like an empty set —
    /// every `ToolName::requires_confirmation` tool still pauses for
    /// approval. Unlike `ai_allowed_tools`, this never widens what a tool
    /// can *do* — it only skips the per-call confirmation prompt for a tool
    /// the user has already vetted on this repo, in this and every future
    /// chat.
    #[serde(default)]
    pub ai_auto_approved_tools: Option<Vec<ToolName>>,
    /// Stable fallback identity for the global embeddings cache
    /// (`~/.atlas/embeddings/{repository_id}`, see `commands::embeddings::
    /// resolve_index_paths`), used only when the repo has no resolvable
    /// canonical remote URL (no git repo, or a git repo with no remotes).
    /// Generated once (a random UUID) and persisted here so the same
    /// project keeps resolving to the same cache folder across sessions.
    #[serde(default)]
    pub local_repository_id: Option<String>,
}

impl ProjectConfig {
    pub fn new(docs_root_relative: impl Into<String>) -> Self {
        Self {
            docs_root: docs_root_relative.into(),
            ai_access_mode: AiAccessMode::default(),
            ai_allowed_tools: None,
            ai_auto_approved_tools: None,
            local_repository_id: None,
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
    #[error("failed to delete path: {0}")]
    Delete(#[source] std::io::Error),
    #[error("failed to rename path: {0}")]
    Rename(#[source] std::io::Error),
    #[error("failed to copy path: {0}")]
    Copy(#[source] std::io::Error),
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
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("invalid name: {0}")]
    InvalidName(String),
    #[error("directory is not empty: {0}")]
    DirectoryNotEmpty(String),
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
pub struct RecentProject {
    pub root: String,
    pub name: String,
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

/// One other document whose `include::`/`image::`/`xref:` references were
/// rewritten as a side effect of a rename/move, and how many changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedReference {
    pub docs_relative_path: String,
    pub count: u32,
}

/// Result of a rename/move that also cascaded into other documents'
/// references — returned instead of `()` so the frontend can refresh any
/// affected open tabs and tell the user what changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RenameReport {
    pub updated_files: Vec<UpdatedReference>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_legacy_project_config_without_ai_access_mode() {
        let config: ProjectConfig = serde_json::from_str(r#"{"docsRoot":"docs"}"#).unwrap();
        assert_eq!(config.docs_root, "docs");
        assert_eq!(config.ai_access_mode, AiAccessMode::DocsOnly);
        assert_eq!(config.ai_allowed_tools, None);
        assert_eq!(config.ai_auto_approved_tools, None);
    }

    #[test]
    fn new_defaults_to_docs_only() {
        let config = ProjectConfig::new("docs");
        assert_eq!(config.ai_access_mode, AiAccessMode::DocsOnly);
        assert_eq!(config.ai_allowed_tools, None);
        assert_eq!(config.ai_auto_approved_tools, None);
    }
}
