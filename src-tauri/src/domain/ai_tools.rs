use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::ai_access::{default_allowed_tools, AiAccessMode, ToolName};
use super::embeddings::EmbeddingError;
use super::paths;
use super::project_config::ProjectError;
use super::repo_index::FileId;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticSearchArgs {
    pub query: String,
    /// `None` falls back to a default (see `services::ai_tools`), clamped
    /// to a hard maximum regardless of what's requested.
    pub top_k: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteFileArgs {
    /// File path relative to the docs root — always the docs subtree,
    /// regardless of `AiAccessMode`, and always restricted to recognized
    /// document file types (see `services::ai_tools::write_file`).
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestFullRepoAccessArgs {
    /// Required, not optional — forces the model to self-justify the
    /// request; shown verbatim in the user-facing approval prompt.
    pub reason: String,
}

/// Which cascade tier produced a `ToolMatch` — scores are only comparable
/// within the same source, never across tiers (see `ToolMatch::score`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchSource {
    Semantic,
    Lexical,
    Symbol,
}

/// One `SemanticSearch` hit. Deliberately not `Deserialize` — nothing ever
/// reconstructs one from the frontend, only serializes one out, mirroring
/// `ToolResult`'s own asymmetry.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolMatch {
    /// Relative to the scope root, `/`-separated — same shape as
    /// `ToolFileEntry::path`.
    pub path: String,
    pub snippet: String,
    /// Only comparable within the same `source`: `Semantic` is
    /// `1.0 - cosine_distance` (higher is better), `Lexical` is a raw
    /// substring-occurrence count, `Symbol` is a fixed `1.0` (exact name
    /// match).
    pub score: f32,
    pub start_byte: u32,
    pub end_byte: u32,
    pub qualified_name: Option<String>,
    pub source: MatchSource,
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
    SemanticSearch(SemanticSearchArgs),
    WriteFile(WriteFileArgs),
    RequestFullRepoAccess(RequestFullRepoAccessArgs),
}

impl ToolCall {
    pub fn name(&self) -> ToolName {
        match self {
            ToolCall::ReadFile(_) => ToolName::ReadFile,
            ToolCall::ListFiles(_) => ToolName::ListFiles,
            ToolCall::SemanticSearch(_) => ToolName::SemanticSearch,
            ToolCall::WriteFile(_) => ToolName::WriteFile,
            ToolCall::RequestFullRepoAccess(_) => ToolName::RequestFullRepoAccess,
        }
    }
}

/// Result of a `ToolCall`. Variants are named after the shape of the
/// payload (a file's content vs. a listing), not after the tool that
/// produced it — mirrors `ToolCall` as the other half of the same
/// serialized boundary.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "tool", content = "result", rename_all = "camelCase")]
pub enum ToolResult {
    File(String),
    FileList(Vec<ToolFileEntry>),
    SemanticSearchResults(Vec<ToolMatch>),
    FileWritten { path: String },
    AccessModeChanged { mode: AiAccessMode },
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
    #[error("semantic search failed: {0}")]
    SemanticSearch(String),
    /// A model-supplied `LlmToolCall::name` that doesn't match any known
    /// `ToolCall` variant — see `services::ai_tools::parse_tool_call`.
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    /// A model-supplied `LlmToolCall::arguments` that doesn't deserialize
    /// into the args struct its `name` maps to (missing/extra/wrong-typed
    /// field, or plain non-JSON) — see `services::ai_tools::parse_tool_call`.
    #[error("invalid arguments for {tool}: {source}")]
    InvalidArguments {
        tool: String,
        #[source]
        source: serde_json::Error,
    },
}

impl From<EmbeddingError> for ToolError {
    fn from(err: EmbeddingError) -> Self {
        ToolError::SemanticSearch(err.to_string())
    }
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
    /// Always `project.root`, regardless of `mode` — the single embedding
    /// index (`RepositoryIndex`/`ChunkIndex`/`EmbeddingIndex`) always covers
    /// the whole repo now, so every `FileId` it hands back is
    /// `repo_root`-relative in both modes. `root` above stays `docs_root` in
    /// `DocsOnly` mode for `ReadFile`/`ListFiles` containment — this field
    /// exists so the three `SemanticSearch` match tiers have the *correct*
    /// root to resolve a `FileId`'s on-disk text against, instead of
    /// (incorrectly) reusing `root`.
    pub repo_root: PathBuf,
    /// Always the real docs subtree, independent of `mode` — unlike `root`,
    /// which is `repo_root` in `FullRepo` mode. `WriteFile` always resolves
    /// against this (never `root`): `FullRepo` grants broader *read*
    /// context so the assistant can write better docs, not license to
    /// mutate arbitrary repo files, so writes stay confined to the docs
    /// subtree regardless of which read boundary is currently active.
    pub docs_root: PathBuf,
    /// `None` when a search result needs no filtering (`FullRepo` mode, or
    /// a `DocsOnly` project whose `docs_root` *is* the repo root) — `Some`
    /// carries `docs_root`'s path relative to `repo_root` (`/`-separated,
    /// e.g. `"docs"`), the same shape `domain::repo_index::FileId` uses, so
    /// `allows_search_result` is a plain string-prefix check
    /// (`paths::is_under_relative_prefix`) rather than a filesystem
    /// resolution per candidate. A `DocsOnly` project whose `docs_root`
    /// somehow isn't under `repo_root` (`paths::relative_to` erroring) falls
    /// back to `Some(String::new())` — fails closed (nothing matches)
    /// rather than silently widening access.
    docs_filter_prefix: Option<String>,
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
        let docs_filter_prefix = match mode {
            AiAccessMode::FullRepo => None,
            AiAccessMode::DocsOnly => match paths::relative_to(repo_root, docs_root) {
                Ok(rel) if rel == "." => None,
                Ok(rel) => Some(rel),
                Err(_) => Some(String::new()),
            },
        };
        Self {
            mode,
            root: root.to_path_buf(),
            repo_root: repo_root.to_path_buf(),
            docs_root: docs_root.to_path_buf(),
            docs_filter_prefix,
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

    /// Whether a `SemanticSearch` match tier may surface `file_id` under
    /// this scope — the query-time counterpart to `root`'s filesystem
    /// containment, needed now that the index behind every tier always
    /// covers the whole repo regardless of `mode`.
    pub fn allows_search_result(&self, file_id: &FileId) -> bool {
        match &self.docs_filter_prefix {
            None => true,
            Some(prefix) => paths::is_under_relative_prefix(&file_id.0, prefix),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// `(repo_root, docs_root)`, both canonicalized real directories —
    /// `ToolScope::new` canonicalizes via `paths::relative_to`, so a
    /// non-existent `docs_root` would hit the fail-closed `Err` branch
    /// rather than the case these tests mean to exercise.
    fn fixture_dirs() -> (PathBuf, PathBuf) {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let repo = std::env::temp_dir().join(format!("alfa-atlas-tool-scope-{nanos}-{n}"));
        let docs = repo.join("docs");
        fs::create_dir_all(&docs).unwrap();
        (repo.canonicalize().unwrap(), docs.canonicalize().unwrap())
    }

    #[test]
    fn full_repo_scope_allows_every_search_result() {
        let (repo, docs) = fixture_dirs();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        assert!(scope.allows_search_result(&FileId("docs/guide.adoc".to_string())));
        assert!(scope.allows_search_result(&FileId("src/Main.java".to_string())));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn docs_only_scope_allows_only_files_under_docs_root() {
        let (repo, docs) = fixture_dirs();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        assert!(scope.allows_search_result(&FileId("docs/guide.adoc".to_string())));
        assert!(scope.allows_search_result(&FileId("docs/nested/page.adoc".to_string())));
        assert!(!scope.allows_search_result(&FileId("src/Main.java".to_string())));
        // A sibling directory that merely shares `docs`' prefix textually
        // must not pass — `is_under_relative_prefix` guards this, not a raw
        // `starts_with("docs")`.
        assert!(!scope.allows_search_result(&FileId("docs-legacy/old.adoc".to_string())));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn docs_only_scope_allows_everything_when_docs_root_is_the_repo_root() {
        let (repo, _docs) = fixture_dirs();
        let scope = ToolScope::for_project(&repo, &repo, AiAccessMode::DocsOnly);

        assert!(scope.allows_search_result(&FileId("src/Main.java".to_string())));

        fs::remove_dir_all(&repo).ok();
    }
}
