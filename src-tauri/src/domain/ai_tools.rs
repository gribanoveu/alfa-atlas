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
    /// Max recursion depth below `path` (`ignore::WalkBuilder`'s own
    /// convention: `path` itself is depth 0, its direct children are depth
    /// 1, so `Some(1)` means direct children only). `Some(0)` is valid —
    /// no descendant entries at all, not an error. `None` = unlimited,
    /// matching the behavior before this field existed.
    pub depth: Option<u32>,
    /// Glob matched against each entry's filename only, never its full
    /// path (e.g. `"*.java"` matches at any depth). Directory entries are
    /// always kept regardless — this scopes which *files* come back, not
    /// the navigable structure.
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFileArgs {
    /// File path relative to the scope root.
    pub path: String,
    /// 1-indexed, inclusive. `None` means "from the beginning of the
    /// file". Out-of-range values are clamped, not rejected — see
    /// `services::ai_tools::slice_lines`.
    pub start_line: Option<u32>,
    /// 1-indexed, inclusive. `None` means "through the end of the file".
    pub end_line: Option<u32>,
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

/// Exact regex content search across files under the tool scope root —
/// read-only, complementary to `SemanticSearch` (precision vs similarity).
/// See `services::ai_tools::grep`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepArgs {
    /// Rust `regex` pattern (no backreferences). Invalid patterns surface
    /// as `ToolError::InvalidPattern`.
    pub pattern: String,
    /// Optional subdirectory relative to the scope root (same as
    /// `ListFilesArgs::path`).
    pub path: Option<String>,
    /// Filename-only glob filter (same semantics as `ListFilesArgs::pattern`).
    pub glob: Option<String>,
    /// Default `false` — exact case-sensitive match unless opted in.
    pub case_insensitive: Option<bool>,
    /// Cap on returned matches; `None` → default, clamped to a hard max
    /// (see `services::ai_tools`).
    pub max_results: Option<usize>,
}

/// One line hit from `grep` — deliberately not `Deserialize` (serialize
/// only), same asymmetry as `ToolMatch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepMatch {
    /// Relative to the scope root, `/`-separated — same shape as
    /// `ToolFileEntry::path` / `readFile` paths.
    pub path: String,
    /// 1-indexed line number.
    pub line: u32,
    /// The matching line's text (possibly truncated for payload size).
    pub text: String,
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

/// One search-and-replace edit within an `EditFileArgs` call. `old` must
/// match the target file's current content exactly once — see
/// `services::ai_tools::apply_edits` for the full validation rules
/// (no match, ambiguous match, and overlapping edits all reject the whole
/// `EditFile` call before anything is written).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEdit {
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditFileArgs {
    /// File path relative to the docs root — same containment/target rules
    /// as `WriteFileArgs::path`, but the file must already exist (see
    /// `services::ai_tools::edit_file`); creating new files stays
    /// `WriteFile`'s job.
    pub path: String,
    pub edits: Vec<FileEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFileArgs {
    /// File path relative to the docs root — same containment/target rules
    /// as `WriteFileArgs::path`. See `services::ai_tools::delete_file`.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDirectoryArgs {
    /// Directory path relative to the docs root — same containment rules
    /// as `WriteFileArgs::path` (always the docs subtree, regardless of
    /// `AiAccessMode`; see `services::ai_tools::create_directory`). Parent
    /// directories are created as needed.
    pub path: String,
    /// Optional folder scaffold. `"restEndpoint"` populates the directory
    /// with the same REST-method template set the Sidebar "Новая папка"
    /// dialog uses (`docs_fs::create_rest_endpoint_folder`). `None`/omit
    /// creates an empty directory.
    #[serde(default)]
    pub template: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteDirectoryArgs {
    /// Directory path relative to the docs root — same containment rules
    /// as `WriteFileArgs::path`.
    pub path: String,
    /// `None`/omitted means `false`: a non-empty directory is refused
    /// rather than silently deleted — see
    /// `services::ai_tools::delete_directory`.
    pub recursive: Option<bool>,
}

/// Covers both moving and renaming, both files and directories — a new
/// `new_path` in the same directory as `path` *is* a rename; a new
/// `new_path` elsewhere *is* a move. Same shape either way, so there is no
/// separate rename tool — see `services::ai_tools::move_path`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveArgs {
    /// Current path, relative to the docs root.
    pub path: String,
    /// New path, relative to the docs root. Fails if something already
    /// exists there.
    pub new_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestFullRepoAccessArgs {
    /// Required, not optional — forces the model to self-justify the
    /// request; shown verbatim in the user-facing approval prompt.
    pub reason: String,
}

/// Working-tree / index / commit file diff for the assistant — read-only.
/// Paths are relative to the tool scope root (`docsRoot` in DocsOnly,
/// `repoRoot` in FullRepo); the executor converts to a repo-relative path
/// after `ensure_under` (see `services::ai_tools::git_diff`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffArgs {
    pub path: String,
    /// `"staged"` or `"unstaged"` — ignored when `commit` is set. `None`
    /// defaults to unstaged (working tree vs index/HEAD).
    pub scope: Option<String>,
    /// Optional commit hash/ref — when set, returns the parent→commit
    /// file diff for `path` instead of a working-tree scope.
    pub commit: Option<String>,
}

/// Line-authorship for one file — read-only. Same path containment as
/// `GitDiffArgs`. Optional line range mirrors `ReadFileArgs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBlameArgs {
    pub path: String,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
}

/// Which verification `check` should run. Extensible — add variants as new
/// check kinds ship; wire name is camelCase (`"problems"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckKind {
    /// Workspace-index diagnostics — the same list BottomDock «Проблемы»
    /// shows (broken xref/include/image, duplicate anchors, circular
    /// includes, parse errors). Only covers supported indexed file types
    /// under the docs root, not arbitrary project source.
    Problems,
}

/// Read-only verification tool — see `services::ai_tools::check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckArgs {
    pub kind: CheckKind,
    /// Optional docs-root-relative file path. `None`/omit = all indexed
    /// documents. Always docs-relative (like `writeFile`), even in
    /// Full-repo mode — diagnostics only cover documentation.
    #[serde(default)]
    pub path: Option<String>,
}

/// A task's lifecycle state. Deliberately only these four — no `failed`,
/// no `blocked`, no `dependsOn`: a failed step is either `Cancelled` with a
/// `note` explaining why, or the agent keeps retrying within the same
/// `InProgress` task. `Pending`/`InProgress` are runtime-only transitions —
/// the model can never set them directly, see `TodoUpdateStatus`, which is
/// the only status shape a `todo update` call can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

/// One entry in the model's working task checklist for the current
/// conversation. Within a single streaming turn it's round-tripped
/// through `ChatStreamOutcome`/`PendingApproval` and owned by the frontend
/// (`useLlmChat`'s `todoListRef`) — the backend's tool-loop itself still
/// keeps no in-memory session state between calls (see `commands::llm`'s
/// tool-loop doc comment). Separately, `infra::chat_store::save_chat`/
/// `load_chat` persist the current list alongside chat messages (the
/// `chats.todos` column), so — unlike the transient per-turn round-trip —
/// it does survive a chat switch, new chat, or app restart; `useLlmChat`
/// just seeds `todoListRef` from that persisted list on mount instead of
/// always starting empty. `id` is runtime-generated (`"t1"`, `"t2"`, ...
/// sequential) — the model never invents or chooses one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub title: String,
    pub status: TodoStatus,
    /// A short result (`Completed`) or the reason (`Cancelled`) — never
    /// required.
    #[serde(default)]
    pub note: Option<String>,
}

/// `op: "write"` — one or more new task titles, appended to the end of the
/// current list (never a replace).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoWriteArgs {
    pub titles: Vec<String>,
}

/// `op: "update"`'s allowed `status` values — deliberately excludes
/// `Pending`/`InProgress` at the type level, which is what makes "the
/// model can never pick the next task itself" a schema-enforced fact
/// rather than a prompt request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TodoUpdateStatus {
    Completed,
    Cancelled,
}

impl From<TodoUpdateStatus> for TodoStatus {
    fn from(s: TodoUpdateStatus) -> Self {
        match s {
            TodoUpdateStatus::Completed => TodoStatus::Completed,
            TodoUpdateStatus::Cancelled => TodoStatus::Cancelled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoUpdateArgs {
    pub id: String,
    pub status: TodoUpdateStatus,
    #[serde(default)]
    pub note: Option<String>,
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
    Grep(GrepArgs),
    GitDiff(GitDiffArgs),
    GitBlame(GitBlameArgs),
    Check(CheckArgs),
    WriteFile(WriteFileArgs),
    EditFile(EditFileArgs),
    DeleteFile(DeleteFileArgs),
    CreateDirectory(CreateDirectoryArgs),
    DeleteDirectory(DeleteDirectoryArgs),
    Move(MoveArgs),
    RequestFullRepoAccess(RequestFullRepoAccessArgs),
    TodoWrite(TodoWriteArgs),
    TodoUpdate(TodoUpdateArgs),
}

impl ToolCall {
    pub fn name(&self) -> ToolName {
        match self {
            ToolCall::ReadFile(_) => ToolName::ReadFile,
            ToolCall::ListFiles(_) => ToolName::ListFiles,
            ToolCall::SemanticSearch(_) => ToolName::SemanticSearch,
            ToolCall::Grep(_) => ToolName::Grep,
            ToolCall::GitDiff(_) => ToolName::GitDiff,
            ToolCall::GitBlame(_) => ToolName::GitBlame,
            ToolCall::Check(_) => ToolName::Check,
            ToolCall::WriteFile(_) => ToolName::WriteFile,
            ToolCall::EditFile(_) => ToolName::EditFile,
            ToolCall::DeleteFile(_) => ToolName::DeleteFile,
            ToolCall::CreateDirectory(_) => ToolName::CreateDirectory,
            ToolCall::DeleteDirectory(_) => ToolName::DeleteDirectory,
            ToolCall::Move(_) => ToolName::Move,
            ToolCall::RequestFullRepoAccess(_) => ToolName::RequestFullRepoAccess,
            ToolCall::TodoWrite(_) => ToolName::Todo,
            ToolCall::TodoUpdate(_) => ToolName::Todo,
        }
    }
}

/// Line-diff summary attached to a settled `FileWritten`/`FileEdited`/
/// `FileDeleted` result — computed once (`services::text_diff::diff_stats`)
/// and consumed by two independent readers: the chat UI's `+N -M` badge and
/// colored diff view, and the model itself, which otherwise only ever saw
/// `{"path": "..."}` and had no way to confirm what actually landed on disk
/// (notably useful for `edit_file`'s fast-apply reconciliation path, where
/// the applied text can differ slightly from what the model asked for).
/// `lines_added`/`lines_removed` are always the true, untruncated totals
/// even when `unified_diff` itself was cut short.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiffStats {
    pub lines_added: u32,
    pub lines_removed: u32,
    /// Unified-diff body only (no `--- a`/`+++ b` file header — the path is
    /// already on the enclosing `ToolResult` variant), capped to
    /// `services::text_diff::MAX_UNIFIED_DIFF_CHARS`.
    pub unified_diff: String,
    pub truncated: bool,
}

/// Result of a `ToolCall`. Variants are named after the shape of the
/// payload (a file's content vs. a listing), not after the tool that
/// produced it — mirrors `ToolCall` as the other half of the same
/// serialized boundary.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "tool", content = "result", rename_all = "camelCase")]
pub enum ToolResult {
    /// `rename_all` on this variant is needed explicitly — the container
    /// attribute above doesn't cascade to a struct variant's own field
    /// names for this adjacently-tagged representation, unlike variant tag
    /// spelling. Struct variants with multi-word fields (`File`,
    /// `DirectoryCreated`, `Moved`, …) set `rename_all = "camelCase"` on the
    /// variant itself — the container attribute does not cascade.
    #[serde(rename_all = "camelCase")]
    File {
        content: String,
        /// 1-indexed, inclusive — the range actually returned (after
        /// clamping), not necessarily what was requested. `0` for both
        /// `start_line`/`end_line` on an empty file (there is no line 1 to
        /// claim).
        start_line: u32,
        end_line: u32,
        total_lines: u32,
    },
    FileList(Vec<ToolFileEntry>),
    SemanticSearchResults(Vec<ToolMatch>),
    /// Settled `grep` — line-oriented regex hits under `scope.root`.
    /// `truncated` is true when the match cap was hit before the walk finished.
    #[serde(rename_all = "camelCase")]
    GrepResults {
        matches: Vec<GrepMatch>,
        truncated: bool,
    },
    /// Settled `gitDiff` — `label` is a short human/model-facing description
    /// of the two sides (e.g. `"HEAD → Working tree"` or a commit short
    /// oid pair). `diff` reuses the same `FileDiffStats` shape write/edit
    /// tools already return so the chat UI can render it with the same
    /// badge + colored unified-diff view.
    #[serde(rename_all = "camelCase")]
    GitDiff {
        path: String,
        label: String,
        diff: FileDiffStats,
        is_binary: bool,
    },
    /// Settled `gitBlame` — contiguous hunks, optionally truncated when
    /// the requested line range exceeds
    /// `services::ai_tools::MAX_BLAME_LINES`.
    #[serde(rename_all = "camelCase")]
    GitBlame {
        path: String,
        hunks: Vec<crate::domain::git::GitBlameHunk>,
        truncated: bool,
    },
    /// Settled `check` — `diagnostics` reuse the same `Diagnostic` wire
    /// shape as the Problems panel (`document` / message paths are
    /// repo-relative). `truncated` when the per-call cap was hit.
    #[serde(rename_all = "camelCase")]
    CheckResults {
        kind: CheckKind,
        diagnostics: Vec<crate::domain::workspace_index::Diagnostic>,
        truncated: bool,
    },
    FileWritten { path: String, diff: FileDiffStats },
    FileEdited { path: String, diff: FileDiffStats },
    FileDeleted { path: String, diff: FileDiffStats },
    /// Settled `createDirectory`. `template`/`created_files` are empty when
    /// the call created a bare folder; with `template: "restEndpoint"` they
    /// report the scaffold that was written (docs-relative paths).
    #[serde(rename_all = "camelCase")]
    DirectoryCreated {
        path: String,
        template: Option<String>,
        created_files: Vec<String>,
    },
    DirectoryDeleted { path: String },
    /// `updated_files` lists every *other* file whose `include::`/`xref:`/
    /// `$ref` references were rewritten as a side effect of this move
    /// (empty when nothing referenced `from`) — the same `RenameReport`
    /// shape the manual rename/move commands return, so the frontend can
    /// reuse the same "reload these open tabs" handling for both. See
    /// `services::ai_tools::move_path`.
    #[serde(rename_all = "camelCase")]
    Moved {
        from: String,
        to: String,
        updated_files: Vec<crate::domain::project_config::UpdatedReference>,
    },
    AccessModeChanged { mode: AiAccessMode },
    /// Full updated list after a `todo write` — both this call's own
    /// result (fed back to the model as a `Tool`-role message) and, via
    /// `commands::llm::run_tool_loop`, what becomes the turn's `todos`
    /// state going forward.
    TodoWritten(Vec<Task>),
    /// Same shape/role as `TodoWritten`, after a `todo update`.
    TodoUpdated(Vec<Task>),
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
    /// A `listFiles` `pattern` that doesn't compile as a glob — see
    /// `services::ai_tools::compile_glob`.
    #[error("invalid glob pattern: {0}")]
    InvalidPattern(String),
    /// An `editFile` edit's `old` text doesn't appear anywhere in the
    /// file's current content — see `services::ai_tools::apply_edits`.
    #[error("edit text not found: {0}")]
    EditTextNotFound(String),
    /// An `editFile` edit's `old` text appears more than once — ambiguous
    /// which occurrence was meant, so nothing is written. `.1` is the match
    /// count.
    #[error("edit text is not unique — matched {1} times: {0}")]
    EditTextAmbiguous(String, usize),
    /// Two `editFile` edits in the same call matched overlapping regions of
    /// the file's original content — applying both would be order-dependent
    /// or corrupt one of them, so the whole call is rejected.
    #[error("edits overlap in the same region of the file")]
    EditsOverlap,
    /// An `editFile` edit's `old` text didn't match exactly (the
    /// `EditTextNotFound`/`EditTextAmbiguous` case), and the fast-apply
    /// reconciliation fallback (`services::ai_tools::run_fast_apply`) — which
    /// hands the whole file plus the edit's intent to the configured LLM
    /// provider and asks it to locate and make the change itself — either
    /// wasn't available (no provider resolved for this call), errored, or
    /// produced output that failed the byte-identical-outside-the-edited-
    /// region safety check (`services::ai_tools::validate_fast_apply_output`).
    /// `.0` is the edit's (possibly truncated) `old` text, `.1` explains why.
    #[error("could not automatically apply edit for \"{0}\": {1}")]
    EditApplyFailed(String, String),
    /// A `deleteDirectory` call with `recursive` omitted or `false` against
    /// a directory that has contents — see
    /// `services::ai_tools::delete_directory`.
    #[error("directory is not empty: {0}")]
    DirectoryNotEmpty(String),
    /// A `move` call whose `newPath` already exists — nothing is
    /// overwritten; `rename_project_file`/`rename_project_dir` check this
    /// before ever calling `fs::rename`. See `services::ai_tools::move_path`.
    #[error("already exists: {0}")]
    AlreadyExists(String),
    /// A model-supplied `LlmToolCall::name` that doesn't match any known
    /// `ToolCall` variant — see `services::ai_tools::parse_tool_call`.
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    /// A model-supplied `LlmToolCall::arguments` that doesn't deserialize
    /// into the args struct its `name` maps to (missing/extra/wrong-typed
    /// field, or plain non-JSON) — see `services::ai_tools::parse_tool_call`.
    /// `reason` is a JSON-path-annotated message (via `serde_path_to_error`,
    /// e.g. `edits[1]: missing field \`old\`` rather than a bare
    /// `serde_json::Error`'s `"... at line 1 column 7275"`) — a real
    /// observed failure is a model getting a field name wrong on one
    /// element deep inside an array argument (e.g. `editFile`'s `edits`);
    /// a byte offset into the raw JSON string doesn't tell it which element
    /// or field to fix, a path does. Plain `String` rather than a `#[source]`
    /// error type — nothing inspects this beyond its rendered text.
    #[error("invalid arguments for {tool}: {reason}")]
    InvalidArguments { tool: String, reason: String },
    /// A `todo write` whose new titles would push the list past
    /// `services::ai_tools::MAX_TODO_TASKS` — rejected outright rather than
    /// silently truncated, so the model sees the failure and can decide
    /// what to drop or split.
    #[error("todo list already has {current} task(s); adding {adding} more would exceed the {max} maximum")]
    TooManyTasks {
        current: usize,
        adding: usize,
        max: usize,
    },
    /// A `todo update` naming an `id` that doesn't exist in the current
    /// list — most likely a stale/hallucinated id from earlier in the
    /// conversation.
    #[error("no task with id: {0}")]
    TaskNotFound(String),
    /// A git read (`gitDiff`/`gitBlame`) failed — wraps `GitError` as a
    /// string so the model sees a useful message without pulling git
    /// types into the tool boundary.
    #[error("git error: {0}")]
    Git(String),
}

impl From<EmbeddingError> for ToolError {
    fn from(err: EmbeddingError) -> Self {
        ToolError::SemanticSearch(err.to_string())
    }
}

impl From<crate::domain::git::GitError> for ToolError {
    fn from(err: crate::domain::git::GitError) -> Self {
        match err {
            crate::domain::git::GitError::InvalidPath(p) => ToolError::PathEscape(p),
            other => ToolError::Git(other.to_string()),
        }
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
            ProjectError::DirectoryNotEmpty(p) => ToolError::DirectoryNotEmpty(p),
            ProjectError::AlreadyExists(p) => ToolError::AlreadyExists(p),
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
