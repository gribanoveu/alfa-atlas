//! Executor for the read-only tools a future AI harness will call. This is
//! the enforcement point for `AiAccessMode`: every function here resolves
//! containment against `scope.root` via `domain::paths` — the same
//! primitives `services::docs_fs` uses — so a caller can never widen access
//! by passing an unexpected path, only by the `ToolScope` itself having been
//! constructed with the wider root.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, TryLockError};

use crate::commands::embeddings::{
    attach_embedding_index, attach_index_store, ensure_provider, resolve_index_paths,
    EmbeddingIndexSlot, EmbeddingProviderSlot, EmbeddingSyncGuard, IndexStoreSlot,
};
use crate::domain::ai_access::{default_allowed_tools, AiAccessMode, ToolName};
use crate::domain::ai_tools::{
    CreateDirectoryArgs, EditFileArgs, FileEdit, ListFilesArgs, MatchSource, ReadFileArgs,
    RequestFullRepoAccessArgs, SemanticSearchArgs, ToolCall, ToolError, ToolFileEntry, ToolMatch,
    ToolResult, ToolScope, WriteFileArgs,
};
use crate::domain::chunk_index::{qualified_name_for, ChunkMetadata};
use crate::domain::llm::{LlmToolCall, LlmToolDefinition};
use crate::domain::paths;
use crate::domain::project_config::{ProjectConfig, ProjectError, TreeNode};
use crate::domain::repo_index::{FileId, Symbol};
use crate::infra::{embedding_credentials_store, embedding_providers, project_store, workspace_scanner};
use crate::services::chunk_builder::ChunkIndex;
use crate::services::chunk_text::resolve_text;
use crate::services::repo_index::RepositoryIndex;
use crate::services::{docs_fs, embedding_config, project_open};

const DEFAULT_TOP_K: usize = 10;
const MAX_TOP_K: usize = 50;
/// Cap on how many characters of matched text land in a `ToolMatch.snippet`
/// — keeps a large chunk's (up to 16KB) full text from blowing up the
/// response payload.
const SNIPPET_MAX_CHARS: usize = 500;

/// The embedding/chunk/repo-index state `SemanticSearch` needs to reach —
/// `execute_tool` is otherwise a pure function with no access to
/// Tauri-managed state. Mirrors exactly what
/// `commands::embeddings::embedding_sync` already receives as
/// `State<'_, Arc<T>>` params; `commands::ai_tools::ai_execute_tool` clones
/// each into this struct before calling `execute_tool`.
pub struct EmbeddingDeps {
    pub repo_index: Arc<RepositoryIndex>,
    pub chunk_index: Arc<ChunkIndex>,
    pub embedding_index: Arc<EmbeddingIndexSlot>,
    pub index_store: Arc<IndexStoreSlot>,
    pub embedding_provider: Arc<EmbeddingProviderSlot>,
    pub sync_guard: Arc<EmbeddingSyncGuard>,
}

#[cfg(test)]
impl EmbeddingDeps {
    /// Fresh, empty instances of every slot — for `ReadFile`/`ListFiles`
    /// tests (which never touch these) and as a base for `SemanticSearch`
    /// tests that need to populate specific state.
    pub fn empty() -> Self {
        Self {
            repo_index: Arc::new(RepositoryIndex::new()),
            chunk_index: Arc::new(ChunkIndex::new()),
            embedding_index: Arc::new(EmbeddingIndexSlot::new(None)),
            index_store: Arc::new(IndexStoreSlot::new(None)),
            embedding_provider: Arc::new(EmbeddingProviderSlot::new(None)),
            sync_guard: Arc::new(EmbeddingSyncGuard::new(())),
        }
    }
}

/// Single entry point for the harness: one allowlist check (via
/// `scope.allows`), one place to serialize a call/result at the LLM
/// boundary (`ToolCall`/`ToolResult` both derive `serde`), and — later —
/// one place to log every tool invocation (not wired up yet).
pub fn execute_tool(
    scope: &ToolScope,
    call: ToolCall,
    deps: &EmbeddingDeps,
) -> Result<ToolResult, ToolError> {
    if !scope.allows(call.name()) {
        return Err(ToolError::NotAllowed(call.name()));
    }
    match call {
        ToolCall::ReadFile(args) => read_file(scope, args).map(|slice| ToolResult::File {
            content: slice.content,
            start_line: slice.start_line,
            end_line: slice.end_line,
            total_lines: slice.total_lines,
        }),
        ToolCall::ListFiles(args) => list_files(scope, args).map(ToolResult::FileList),
        ToolCall::SemanticSearch(args) => {
            semantic_search(scope, args, deps).map(ToolResult::SemanticSearchResults)
        }
        ToolCall::WriteFile(args) => {
            write_file(scope, args).map(|path| ToolResult::FileWritten { path })
        }
        ToolCall::EditFile(args) => {
            edit_file(scope, args).map(|path| ToolResult::FileEdited { path })
        }
        ToolCall::CreateDirectory(args) => {
            create_directory(scope, args).map(|path| ToolResult::DirectoryCreated { path })
        }
        ToolCall::RequestFullRepoAccess(_args) => set_access_mode(AiAccessMode::FullRepo)
            .map(|()| ToolResult::AccessModeChanged { mode: AiAccessMode::FullRepo })
            .map_err(ToolError::from),
    }
}

/// Persists a new `AiAccessMode` for the currently open project — shared by
/// the manual `commands::ai_tools::ai_set_access_mode` toggle and the
/// `RequestFullRepoAccess` tool, so a mode change behaves identically
/// regardless of which path triggered it. Preserves any existing
/// `ai_allowed_tools` override rather than resetting it.
pub fn set_access_mode(mode: AiAccessMode) -> Result<(), ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let mut config = project_store::load(&opened.root)?
        .unwrap_or_else(|| ProjectConfig::new(opened.docs_root.clone()));
    config.ai_access_mode = mode;
    project_store::save(&opened.root, &config)
}

/// Resolves a `ToolScope` from a project's persisted config — the one place
/// that turns "user hasn't customized anything" into `mode`'s default
/// allowlist, and a customized list into the authoritative one.
pub fn scope_for_config(repo_root: &Path, docs_root: &Path, config: &ProjectConfig) -> ToolScope {
    let allowed: HashSet<ToolName> = config
        .ai_allowed_tools
        .clone()
        .map(|v| v.into_iter().collect())
        .unwrap_or_else(|| default_allowed_tools(config.ai_access_mode));
    ToolScope::new(repo_root, docs_root, config.ai_access_mode, allowed)
}

/// Resolves a `ToolScope` for whichever project is currently open, without
/// the caller (the IPC command) supplying any path — this is what lets the
/// frontend call `ai_execute_tool` knowing nothing about `docsRoot`/
/// `repoRoot`/the access mode. Reuses the same backend-authoritative source
/// `commands::project::get_project` already uses at startup restore;
/// `project_open::get_project()` alone doesn't expose `ai_access_mode`/
/// `ai_allowed_tools` (it discards the rest of `ProjectConfig`), so those
/// are loaded separately here.
pub fn current_scope() -> Result<ToolScope, ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let config = project_store::load(&opened.root)?
        .unwrap_or_else(|| ProjectConfig::new(opened.docs_root.clone()));
    Ok(scope_for_config(
        Path::new(&opened.root),
        Path::new(&opened.docs_root),
        &config,
    ))
}

/// Parses a model-supplied `LlmToolCall` into a concrete `ToolCall` — the
/// step `domain::llm::LlmToolCall`'s own doc comment names this module as
/// the intended home for. A model can send a `name` this executor doesn't
/// recognize, or `arguments` that don't deserialize into the matching args
/// struct (a hallucinated field, wrong type, plain non-JSON); both are
/// `ToolError` variants meant to be fed back to the model as a `Tool`-role
/// message (see `commands::llm::llm_chat_stream`), not to hard-fail the
/// whole turn — a model recovering from a bad tool call of its own making
/// is normal, expected tool-calling behavior.
pub fn parse_tool_call(call: &LlmToolCall) -> Result<ToolCall, ToolError> {
    match call.name.as_str() {
        "readFile" => lenient_json_object::<ReadFileArgs>(&call.arguments)
            .map(ToolCall::ReadFile)
            .map_err(|source| ToolError::InvalidArguments { tool: call.name.clone(), source }),
        "listFiles" => lenient_json_object::<ListFilesArgs>(&call.arguments)
            .map(ToolCall::ListFiles)
            .map_err(|source| ToolError::InvalidArguments { tool: call.name.clone(), source }),
        "semanticSearch" => lenient_json_object::<SemanticSearchArgs>(&call.arguments)
            .map(ToolCall::SemanticSearch)
            .map_err(|source| ToolError::InvalidArguments { tool: call.name.clone(), source }),
        "writeFile" => lenient_json_object::<WriteFileArgs>(&call.arguments)
            .map(ToolCall::WriteFile)
            .map_err(|source| ToolError::InvalidArguments { tool: call.name.clone(), source }),
        "editFile" => lenient_json_object::<EditFileArgs>(&call.arguments)
            .map(ToolCall::EditFile)
            .map_err(|source| ToolError::InvalidArguments { tool: call.name.clone(), source }),
        "createDirectory" => lenient_json_object::<CreateDirectoryArgs>(&call.arguments)
            .map(ToolCall::CreateDirectory)
            .map_err(|source| ToolError::InvalidArguments { tool: call.name.clone(), source }),
        "requestFullRepoAccess" => lenient_json_object::<RequestFullRepoAccessArgs>(&call.arguments)
            .map(ToolCall::RequestFullRepoAccess)
            .map_err(|source| ToolError::InvalidArguments { tool: call.name.clone(), source }),
        other => Err(ToolError::UnknownTool(other.to_string())),
    }
}

/// Parses `arguments` tolerating trailing garbage *after* an otherwise
/// complete, valid JSON object — a real, observed failure mode: some
/// providers stream a tool call's `arguments` as multiple fragments that
/// get concatenated (see `infra::llm_providers::openai_compatible::
/// ToolCallAccumulator`), and at least one has been seen appending a
/// spurious extra fragment after the object already closed (e.g. `{}` then
/// a stray `""`, producing `{}""` — plain `serde_json::from_str` rejects
/// this as "trailing characters" even though the object itself is fine).
///
/// `serde_json::from_str` calls the deserializer's `end()`, which fails on
/// any leftover non-whitespace input; deserializing directly from a
/// `Deserializer` without calling `end()` stops as soon as one complete
/// value has been read and simply ignores whatever follows. This only
/// rescues "valid value + garbage" — a genuinely malformed object (missing
/// braces, invalid syntax partway through) still fails exactly as before,
/// so a model that sends truly broken JSON still gets an honest error to
/// learn from.
fn lenient_json_object<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, serde_json::Error> {
    match serde_json::from_str::<T>(input) {
        Ok(value) => Ok(value),
        Err(strict_err) => {
            let mut de = serde_json::Deserializer::from_str(input);
            T::deserialize(&mut de).map_err(|_| strict_err)
        }
    }
}

/// One `LlmToolDefinition` per tool `scope` allows, to advertise to the
/// model — so a customized (narrowed) allowlist only offers tools that will
/// actually succeed if called, rather than the model discovering
/// `ToolError::NotAllowed` only at execution time. Wire tag values
/// (`"readFile"`/`"listFiles"`/`"semanticSearch"`) and argument field names
/// (`path`, `startLine`+`endLine`, `depth`+`pattern`, `query`+`topK`) are
/// hand-kept in sync with `ToolCall`/`ReadFileArgs`/`ListFilesArgs`/
/// `SemanticSearchArgs` — see this module's schema round-trip test, which
/// catches drift between the two.
pub fn llm_tool_definitions(scope: &ToolScope) -> Vec<LlmToolDefinition> {
    let mut defs = Vec::new();
    if scope.allows(ToolName::ListFiles) {
        defs.push(LlmToolDefinition {
            name: "listFiles".to_string(),
            description: "List files and directories under a path in the current workspace. The workspace root is already configured by the application; `path` is relative to that workspace root. Omit `path` or pass null to list the workspace root. Use this tool to discover files, locate files, or inspect directories."
    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": ["string", "null"],
                        "description": "Subdirectory relative to the project root, or omitted/null for the root."
                    },
                    "depth": {
                        "type": ["integer", "null"],
                        "minimum": 0,
                        "description": "Maximum recursion depth below `path` (1 = only direct children, 0 = no descendant entries at all). Omit or null for no limit."
                    },
                    "pattern": {
                        "type": ["string", "null"],
                        "description": "Glob pattern (e.g. \"*.java\") matched against each entry's filename only, not its full path. Directories are always included regardless of this filter. Omit or null for no filtering."
                    }
                },
                "required": []
            }),
        });
    }
    if scope.allows(ToolName::ReadFile) {
        defs.push(LlmToolDefinition {
            name: "readFile".to_string(),
            description: "Read the text content of one file by its path relative to the project root, optionally restricted to a line range. Use when the relevant file is already known, exact content is required, a search result needs verification, or a claim depends on specific implementation or documentation details. Prefer a line range for a large file when only part of it is relevant."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the project root."
                    },
                    "startLine": {
                        "type": ["integer", "null"],
                        "minimum": 1,
                        "description": "1-indexed first line to return (inclusive). Omit or null to start from the beginning of the file."
                    },
                    "endLine": {
                        "type": ["integer", "null"],
                        "minimum": 1,
                        "description": "1-indexed last line to return (inclusive). Omit or null to read through the end of the file. Out-of-range values are clamped, not rejected."
                    }
                },
                "required": ["path"]
            }),
        });
    }
    if scope.allows(ToolName::SemanticSearch) {
        defs.push(LlmToolDefinition {
            name: "semanticSearch".to_string(),
            description:
                "Search the project's documentation/code for content relevant to a natural-language query. Use for semantic discovery: finding documents related to a concept, locating terminology, finding related implementations, or discovering potentially relevant files when the exact location is unknown. Results are useful for discovery but may not be sufficient evidence for precise claims — verify with readFile when precise details matter."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural-language search query."
                    },
                    "topK": {
                        "type": ["integer", "null"],
                        "minimum": 1,
                        "description": "Max number of results, default 10, capped at 50."
                    }
                },
                "required": ["query"]
            }),
        });
    }
    if scope.allows(ToolName::WriteFile) {
        defs.push(LlmToolDefinition {
            name: "writeFile".to_string(),
            description:
                "Create or overwrite one documentation file's full content, given its path relative to the project root. Any missing parent directories in the path are created automatically — there is no need to call createDirectory first. Always requires explicit user approval before the write actually happens — the user may deny it, in which case the file is left unchanged. Do not retry automatically after a denial; ask the user how they'd like to proceed instead. Only recognized documentation file types can be written."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the project root. Must be a recognized documentation file type."
                    },
                    "content": {
                        "type": "string",
                        "description": "The full new content of the file, replacing any existing content."
                    }
                },
                "required": ["path", "content"]
            }),
        });
    }
    if scope.allows(ToolName::EditFile) {
        defs.push(LlmToolDefinition {
            name: "editFile".to_string(),
            description:
                "Make one or more precise, targeted edits to an existing documentation file by replacing exact snippets of its current content, given its path relative to the project root. Each edit's `old` text must appear in the file's CURRENT content exactly once — if it's missing or appears more than once, the entire call is rejected and nothing is written; add a few more surrounding lines to `old` to make it unique rather than guessing. All edits in one call are validated against the file's original content and applied together, or none are — they are independent of each other and of their order. Prefer this over writeFile for small, localized changes: it's cheaper and safer than resending the whole file. Always requires explicit user approval before anything is written."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the project root. Must be a recognized documentation file type and must already exist."
                    },
                    "edits": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "old": {
                                    "type": "string",
                                    "description": "Exact text to find — must appear exactly once in the file's current content."
                                },
                                "new": {
                                    "type": "string",
                                    "description": "Text to replace it with."
                                }
                            },
                            "required": ["old", "new"]
                        },
                        "description": "One or more find-and-replace edits, applied together against the file's original content, not sequentially against each other's output."
                    }
                },
                "required": ["path", "edits"]
            }),
        });
    }
    if scope.allows(ToolName::CreateDirectory) {
        defs.push(LlmToolDefinition {
            name: "createDirectory".to_string(),
            description:
                "Create a directory (including any missing parent directories) given its path relative to the project root. Use this before writing a file into a folder that doesn't exist yet. Always requires explicit user approval before it actually happens — the user may deny it, in which case nothing is created. Fails if the path already exists as a file or directory."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path relative to the project root."
                    }
                },
                "required": ["path"]
            }),
        });
    }
    if scope.allows(ToolName::RequestFullRepoAccess) {
        defs.push(LlmToolDefinition {
            name: "requestFullRepoAccess".to_string(),
            description:
                "Request escalating from docs-only to full-repo access when repository access beyond documentation is genuinely needed to answer the user's request. Requires a stated reason, and always requires explicit user approval — the user may deny it. Do not call this speculatively or repeatedly; only when docs-only access is clearly insufficient."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "reason": {
                        "type": "string",
                        "description": "Why full-repo access is needed for the current request."
                    }
                },
                "required": ["reason"]
            }),
        });
    }
    defs
}

fn list_files(scope: &ToolScope, args: ListFilesArgs) -> Result<Vec<ToolFileEntry>, ToolError> {
    let subdir = resolve_subdir(scope, args.path.as_deref())?;

    let mut entries = match scope.mode {
        AiAccessMode::DocsOnly => list_docs_only(scope, subdir.as_ref(), args.depth)?,
        AiAccessMode::FullRepo => {
            list_full_repo(scope, subdir.map(|(_, abs)| abs), args.depth)?
        }
    };

    if let Some(pattern) = args.pattern.as_deref() {
        let matcher = compile_glob(pattern)?;
        // Directories are always kept — `pattern` scopes which *files*
        // come back, not the navigable structure. Moot in `FullRepo` mode
        // anyway: `list_full_repo` never returns directory entries.
        entries.retain(|e| e.is_dir || matcher.is_match(basename(&e.path)));
    }

    Ok(entries)
}

/// `ToolFileEntry::path` is always `/`-separated by construction
/// (`paths::relative_to`), so a plain `rsplit` avoids any
/// `std::path::Path`/OsStr platform quirks.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn compile_glob(pattern: &str) -> Result<globset::GlobMatcher, ToolError> {
    globset::Glob::new(pattern)
        .map(|g| g.compile_matcher())
        .map_err(|e| ToolError::InvalidPattern(e.to_string()))
}

/// One `readFile` result: a possibly-partial slice of a file's lines,
/// along with enough range/total metadata for the model to know it's
/// looking at less than the whole file.
struct FileSlice {
    content: String,
    start_line: u32,
    end_line: u32,
    total_lines: u32,
}

fn read_file(scope: &ToolScope, args: ReadFileArgs) -> Result<FileSlice, ToolError> {
    // No extension filtering here, unlike `docs_fs::read_project_file` —
    // the tool boundary is containment under `scope.root` alone. In
    // `FullRepo` mode the harness must be able to read source files, which
    // aren't in `is_supported_file`'s doc-format list.
    let joined = paths::join_relative(&scope.root, &args.path)?;
    let canonical = paths::ensure_under(&scope.root, &joined)?;
    if !canonical.exists() {
        return Err(ToolError::NotFound(args.path));
    }
    if !canonical.is_file() {
        return Err(ToolError::NotAFile(args.path));
    }
    let content = fs::read_to_string(&canonical).map_err(ToolError::Io)?;
    Ok(slice_lines(content, args.start_line, args.end_line))
}

/// Clamps `start_line`/`end_line` into range rather than erroring (mirrors
/// `SemanticSearchArgs.top_k`'s `.clamp(1, MAX_TOP_K)` handling below).
/// When neither is requested, `content` is returned byte-identical to what
/// `fs::read_to_string` produced — no split/rejoin round trip for the
/// common full-file case. An empty file reports `start_line: 0,
/// end_line: 0, total_lines: 0` (there is no line 1 to claim). If
/// `end_line` clamps below `start_line` after each is independently
/// clamped into `[1, total_lines]`, `end_line` is raised to `start_line`
/// (returns that one line) rather than erroring.
fn slice_lines(content: String, start_line: Option<u32>, end_line: Option<u32>) -> FileSlice {
    if start_line.is_none() && end_line.is_none() {
        let total_lines = content.lines().count() as u32;
        let start_line = if total_lines == 0 { 0 } else { 1 };
        return FileSlice { content, start_line, end_line: total_lines, total_lines };
    }
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len() as u32;
    if total_lines == 0 {
        return FileSlice { content: String::new(), start_line: 0, end_line: 0, total_lines: 0 };
    }
    let start = start_line.unwrap_or(1).clamp(1, total_lines);
    let end = end_line.unwrap_or(total_lines).clamp(start, total_lines);
    let mut sliced = lines[(start - 1) as usize..end as usize].join("\n");
    sliced.push('\n');
    FileSlice { content: sliced, start_line: start, end_line: end, total_lines }
}

/// Always targets `scope.docs_root`, never `scope.root` — unlike
/// `read_file`, this is deliberately mode-independent: `FullRepo` widens
/// what the assistant can *read* for context, not what it may write.
/// Reuses `docs_fs::write_project_file` as-is: create-or-overwrite, creates
/// parent directories, and rejects anything outside the recognized
/// document-format allowlist (`domain::supported_files::is_supported_file`)
/// regardless of `AiAccessMode`.
fn write_file(scope: &ToolScope, args: WriteFileArgs) -> Result<String, ToolError> {
    docs_fs::write_project_file(&scope.docs_root.to_string_lossy(), &args.path, &args.content)?;
    Ok(args.path)
}

/// Same `scope.docs_root`-always targeting as `write_file`, but composes
/// `docs_fs::read_project_file` + `apply_edits` + `docs_fs::
/// write_project_file` instead of taking new content directly — the file
/// must already exist (a missing file surfaces `read_project_file`'s own
/// `NotFound`, converted via `ToolError`'s `From<ProjectError>`); creating
/// new files stays `write_file`'s job.
fn edit_file(scope: &ToolScope, args: EditFileArgs) -> Result<String, ToolError> {
    let docs_root = scope.docs_root.to_string_lossy();
    let content = docs_fs::read_project_file(&docs_root, &args.path)?;
    let edited = apply_edits(&content, &args.edits)?;
    docs_fs::write_project_file(&docs_root, &args.path, &edited)?;
    Ok(args.path)
}

/// Applies every edit in `edits` to `content` in one all-or-nothing pass.
/// Each edit's `old` is looked up in `content` *as given* — never against
/// the output of an earlier edit in the same call — so edits in one
/// `editFile` call are independent of each other and of their own order.
/// Fails fast on the first problem found (checked per-edit in order, then
/// across all edits together): `old` missing from `content` at all
/// (`EditTextNotFound`), `old` appearing more than once (`EditTextAmbiguous`,
/// ambiguous which occurrence was meant), or two edits' matched regions
/// overlapping (`EditsOverlap`, which would make the result order-dependent
/// or corrupt one of them). Nothing is written by this function itself —
/// it only ever returns the would-be new content; `edit_file` is what
/// actually persists it, and only once every edit here has validated clean.
fn apply_edits(content: &str, edits: &[FileEdit]) -> Result<String, ToolError> {
    let mut ranges: Vec<(usize, usize, &str)> = Vec::with_capacity(edits.len());
    for edit in edits {
        let mut occurrences = content.match_indices(edit.old.as_str());
        let Some((start, _)) = occurrences.next() else {
            return Err(ToolError::EditTextNotFound(edit.old.clone()));
        };
        let count = 1 + occurrences.count();
        if count > 1 {
            return Err(ToolError::EditTextAmbiguous(edit.old.clone(), count));
        }
        ranges.push((start, start + edit.old.len(), edit.new.as_str()));
    }

    ranges.sort_by_key(|&(start, _, _)| start);
    for pair in ranges.windows(2) {
        let (_, prev_end, _) = pair[0];
        let (next_start, _, _) = pair[1];
        if next_start < prev_end {
            return Err(ToolError::EditsOverlap);
        }
    }

    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;
    for (start, end, new) in ranges {
        result.push_str(&content[cursor..start]);
        result.push_str(new);
        cursor = end;
    }
    result.push_str(&content[cursor..]);
    Ok(result)
}

/// Always targets `scope.docs_root`, same reasoning as `write_file` —
/// `FullRepo` widens what the assistant can read for context, not where it
/// may create directories. Reuses `docs_fs::create_project_dir` as-is:
/// creates missing parent directories, fails if the path already exists
/// (as either a file or a directory).
fn create_directory(scope: &ToolScope, args: CreateDirectoryArgs) -> Result<String, ToolError> {
    docs_fs::create_project_dir(&scope.docs_root.to_string_lossy(), &args.path)?;
    Ok(args.path)
}

/// Validates an optional subdirectory argument once, shared by both mode
/// branches: returns its root-relative string form (for the docs-only
/// prefix filter) and its canonical absolute form (for the full-repo scan
/// root).
fn resolve_subdir(
    scope: &ToolScope,
    path: Option<&str>,
) -> Result<Option<(String, PathBuf)>, ToolError> {
    let Some(path) = path else {
        return Ok(None);
    };
    if path.is_empty() || path == "." {
        return Ok(None);
    }
    let joined = paths::join_relative(&scope.root, path)?;
    let canonical = paths::ensure_under(&scope.root, &joined)?;
    if !canonical.is_dir() {
        return Err(ToolError::NotFound(path.to_string()));
    }
    let rel = paths::relative_to(&scope.root, &canonical)?;
    Ok(Some((rel, canonical)))
}

fn list_docs_only(
    scope: &ToolScope,
    subdir: Option<&(String, PathBuf)>,
    max_depth: Option<u32>,
) -> Result<Vec<ToolFileEntry>, ToolError> {
    let dir = subdir.map(|(_, abs)| abs.as_path()).unwrap_or(scope.root.as_path());
    let tree = docs_fs::list_docs_tree_scoped(&scope.root, dir, max_depth)?;
    let mut entries = Vec::new();
    flatten_tree(tree, &mut entries);
    Ok(entries)
}

fn flatten_tree(nodes: Vec<TreeNode>, out: &mut Vec<ToolFileEntry>) {
    for node in nodes {
        out.push(ToolFileEntry {
            path: node.path,
            is_dir: node.is_dir,
        });
        if let Some(children) = node.children {
            flatten_tree(children, out);
        }
    }
}

fn list_full_repo(
    scope: &ToolScope,
    scan_root: Option<PathBuf>,
    max_depth: Option<u32>,
) -> Result<Vec<ToolFileEntry>, ToolError> {
    let scan_root = scan_root.unwrap_or_else(|| scope.root.clone());
    let files = workspace_scanner::scan_all_with_depth(&scan_root, max_depth.map(|d| d as usize))?;
    files
        .into_iter()
        .map(|f| {
            let rel = paths::relative_to(&scope.root, &f.path)?;
            Ok(ToolFileEntry {
                path: rel,
                is_dir: false,
            })
        })
        .collect()
}

/// Cascade entry point: an exact symbol-name hit (cheapest, always tried)
/// is prepended to whichever of the semantic/lexical tiers fills the
/// remaining `top_k` budget, chosen by `is_semantic_ready`.
fn semantic_search(
    scope: &ToolScope,
    args: SemanticSearchArgs,
    deps: &EmbeddingDeps,
) -> Result<Vec<ToolMatch>, ToolError> {
    let top_k = args.top_k.unwrap_or(DEFAULT_TOP_K).clamp(1, MAX_TOP_K);

    let mut results = symbol_matches(&deps.repo_index, scope, &args.query, top_k);

    let remaining = top_k.saturating_sub(results.len());
    if remaining == 0 {
        return Ok(results);
    }

    if is_semantic_ready(deps) {
        results.extend(semantic_matches(scope, deps, &args.query, remaining)?);
    } else {
        results.extend(lexical_matches(&deps.chunk_index, scope, &args.query, remaining));
    }
    Ok(results)
}

/// Mirrors `commands::embeddings::embedding_index_status`'s readiness check
/// exactly (`resolve_index_paths` -> `attach_index_store` -> stale check ->
/// `attach_embedding_index(allow_repair: false)` -> `embedded_count > 0`),
/// plus a `try_lock` peek at `EmbeddingSyncGuard` for "a sync is actively
/// running right now". The guard is never held through this check or the
/// search that follows — its `try_lock` guard value is dropped immediately
/// (never bound to a variable), matching `embedding_index_status`'s own
/// precedent of never acquiring this guard at all for a read. Any failure
/// along the rest of this sequence (no project open, a transient
/// store-open error) degrades to "not ready" rather than propagating —
/// consistent with the whole feature being a graceful cascade, not a
/// pipeline that should hard-fail just because the fast path had a hiccup.
fn is_semantic_ready(deps: &EmbeddingDeps) -> bool {
    // `WouldBlock` (a sync is actively running right now) is the only
    // `try_lock` outcome that should degrade this call — `Poisoned` must
    // not, or a single panic elsewhere while holding this guard (see
    // `commands::embeddings::lock_sync_guard`'s doc comment) would disable
    // semantic search for the rest of the app's lifetime instead of just
    // this one call.
    if matches!(deps.sync_guard.try_lock(), Err(TryLockError::WouldBlock)) {
        return false;
    }

    let Ok(Some(project)) = project_open::get_project() else {
        return false;
    };
    let Ok((index_root, storage_dir)) = resolve_index_paths(&project) else {
        return false;
    };
    let Ok((store, stale)) =
        attach_index_store(&deps.chunk_index, &deps.index_store, &storage_dir, &index_root)
    else {
        return false;
    };
    if stale {
        return false;
    }

    let Ok(config) = embedding_config::load_embedding_config() else {
        return false;
    };
    let dimensions = embedding_providers::expected_dimensions(&config);
    if attach_embedding_index(&deps.embedding_index, &store, &index_root, dimensions, false).is_err()
    {
        return false;
    }

    let Ok(slot) = deps.embedding_index.lock() else {
        return false;
    };
    slot.as_ref().is_some_and(|(_, _, index)| index.len() > 0)
}

/// Embeds `query`, searches the resident `EmbeddingIndex`, and resolves
/// each hit's chunk text. Independently re-derives `index_root`/attaches
/// the store/index rather than reusing `is_semantic_ready`'s work — cheap
/// and idempotent (each attach short-circuits when already current),
/// matching how `embedding_sync`/`embedding_index_status` each separately
/// re-resolve this instead of sharing state across calls.
fn semantic_matches(
    scope: &ToolScope,
    deps: &EmbeddingDeps,
    query: &str,
    top_k: usize,
) -> Result<Vec<ToolMatch>, ToolError> {
    let project = project_open::get_project()
        .map_err(|e| ToolError::SemanticSearch(e.to_string()))?
        .ok_or_else(|| ToolError::SemanticSearch("no project is open".to_string()))?;
    let (index_root, storage_dir) =
        resolve_index_paths(&project).map_err(ToolError::SemanticSearch)?;
    let (store, _stale) = attach_index_store(
        &deps.chunk_index,
        &deps.index_store,
        &storage_dir,
        &index_root,
    )
    .map_err(ToolError::SemanticSearch)?;

    let config = embedding_config::load_embedding_config()
        .map_err(|e| ToolError::SemanticSearch(e.to_string()))?;
    let dimensions = embedding_providers::expected_dimensions(&config);
    attach_embedding_index(&deps.embedding_index, &store, &index_root, dimensions, false)
        .map_err(ToolError::SemanticSearch)?;

    let api_key = embedding_credentials_store::get_api_key();
    let provider = ensure_provider(&deps.embedding_provider, &config, api_key)
        .map_err(ToolError::SemanticSearch)?;

    let query_embedding = provider
        .embed(&[query])?
        .into_iter()
        .next()
        .ok_or_else(|| {
            ToolError::SemanticSearch("embedding provider returned no vector".to_string())
        })?;

    let hits = {
        let slot = deps.embedding_index.lock().map_err(|_| {
            ToolError::SemanticSearch("embedding index lock poisoned".to_string())
        })?;
        let Some((_, _, index)) = slot.as_ref() else {
            return Ok(Vec::new());
        };
        // This `usearch` wrapper has no predicate-aware ANN search — when
        // this scope filters results (`DocsOnly`), over-fetch the whole
        // corpus so filtering below can still fill `top_k` from whatever's
        // left, rather than silently returning fewer hits than the caller
        // asked for just because the nearest raw neighbors happened to be
        // outside `docs_root`.
        let search_k = if scope.mode == AiAccessMode::DocsOnly {
            top_k.max(index.len())
        } else {
            top_k
        };
        index.search(&query_embedding, search_k)?
    };

    let mut out = Vec::with_capacity(top_k.min(hits.len()));
    for (chunk_id, distance) in hits {
        if out.len() >= top_k {
            break;
        }
        let Some(metadata) = deps.chunk_index.get(&chunk_id) else {
            continue;
        };
        if !scope.allows_search_result(&metadata.file_id) {
            continue;
        }
        let Ok(text) = resolve_text(&scope.repo_root, &metadata) else {
            continue;
        };
        out.push(ToolMatch {
            path: metadata.file_id.0,
            snippet: truncate_snippet(&text),
            // `EmbeddingIndex::search` returns cosine distance (lower is
            // closer) — flip to a "higher is better" similarity score.
            score: 1.0 - distance,
            start_byte: metadata.start_byte,
            end_byte: metadata.end_byte,
            qualified_name: metadata.qualified_name,
            source: MatchSource::Semantic,
        });
    }
    Ok(out)
}

/// No-embeddings fallback: scans every chunk's resolved text for a
/// case-insensitive substring match, ranked by occurrence count (a weak
/// proxy score — not comparable to the semantic tier's cosine similarity).
fn lexical_matches(
    chunk_index: &ChunkIndex,
    scope: &ToolScope,
    query: &str,
    top_k: usize,
) -> Vec<ToolMatch> {
    let needle = query.to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(usize, ChunkMetadata, String)> = Vec::new();
    for metadata in chunk_index.all() {
        if !scope.allows_search_result(&metadata.file_id) {
            continue;
        }
        let Ok(text) = resolve_text(&scope.repo_root, &metadata) else {
            continue;
        };
        let count = text.to_lowercase().matches(&needle).count();
        if count > 0 {
            scored.push((count, metadata, text));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.truncate(top_k);

    scored
        .into_iter()
        .map(|(count, metadata, text)| ToolMatch {
            path: metadata.file_id.0,
            snippet: truncate_snippet(&text),
            score: count as f32,
            start_byte: metadata.start_byte,
            end_byte: metadata.end_byte,
            qualified_name: metadata.qualified_name,
            source: MatchSource::Lexical,
        })
        .collect()
}

/// Cheapest tier: an exact (case-insensitive) symbol-name match, no disk
/// I/O beyond a best-effort snippet read. Always tried first, regardless
/// of whether the embedding index is ready.
fn symbol_matches(
    repo_index: &RepositoryIndex,
    scope: &ToolScope,
    query: &str,
    top_k: usize,
) -> Vec<ToolMatch> {
    repo_index
        .find_symbol(query)
        .into_iter()
        .filter(|(file_id, _)| scope.allows_search_result(file_id))
        .take(top_k)
        .filter_map(|(file_id, symbol)| {
            let all_symbols = repo_index.get(&file_id)?.symbols;
            let qualified_name =
                qualified_name_for(&symbol, &all_symbols).or_else(|| Some(symbol.name.clone()));
            let snippet =
                read_symbol_snippet(&scope.repo_root, &file_id, &symbol).unwrap_or_default();
            Some(ToolMatch {
                path: file_id.0,
                snippet,
                score: 1.0,
                start_byte: symbol.start_byte,
                end_byte: symbol.end_byte,
                qualified_name,
                source: MatchSource::Symbol,
            })
        })
        .collect()
}

/// Best-effort slice of `[symbol.start_byte..symbol.end_byte)` off whatever
/// is on disk right now. Unlike `chunk_text::resolve_text`, there's no
/// per-symbol content hash to check staleness against (`Symbol` carries no
/// hash, only `IndexedFile.metadata.hash` does, at the whole-file level) —
/// this simply returns `None` (dropping the snippet, not the whole match)
/// if the byte range is no longer valid for the file's current content.
fn read_symbol_snippet(scope_root: &Path, file_id: &FileId, symbol: &Symbol) -> Option<String> {
    let path = scope_root.join(&file_id.0);
    let content = fs::read_to_string(&path).ok()?;
    let start = symbol.start_byte as usize;
    let end = symbol.end_byte as usize;
    if end > content.len()
        || start > end
        || !content.is_char_boundary(start)
        || !content.is_char_boundary(end)
    {
        return None;
    }
    Some(truncate_snippet(&content[start..end]))
}

fn truncate_snippet(text: &str) -> String {
    if text.len() <= SNIPPET_MAX_CHARS {
        return text.to_string();
    }
    let mut end = SNIPPET_MAX_CHARS;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Builds a `repo_root/docs/...` + `repo_root/src/...` fixture and
    /// returns `(repo_root, docs_root)`, both canonicalized. This file has
    /// far more parallel fixture-based tests than a nanosecond timestamp
    /// alone reliably disambiguates on a coarser system clock — the counter
    /// guarantees uniqueness within the process regardless of clock
    /// resolution.
    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture_repo() -> (PathBuf, PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let repo = std::env::temp_dir().join(format!("alfa-atlas-ai-tools-{nanos}-{n}"));
        let docs = repo.join("docs");
        let src = repo.join("src");
        fs::create_dir_all(&docs).unwrap();
        fs::create_dir_all(&src).unwrap();
        fs::write(docs.join("intro.adoc"), "= Intro\n").unwrap();
        fs::write(docs.join("script.py"), "print('unsupported ext')\n").unwrap();
        fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();

        let repo = repo.canonicalize().unwrap();
        let docs = docs.canonicalize().unwrap();
        (repo, docs)
    }

    /// Calls `execute_tool` for `ReadFile` and unwraps the expected
    /// `ToolResult::File` shape, so tests read like the plain `read_file`
    /// calls they replaced while still exercising the real public entry
    /// point (allowlist check included).
    fn read(scope: &ToolScope, path: &str) -> Result<String, ToolError> {
        match execute_tool(
            scope,
            ToolCall::ReadFile(ReadFileArgs {
                path: path.to_string(),
                start_line: None,
                end_line: None,
            }),
            &EmbeddingDeps::empty(),
        )? {
            ToolResult::File { content, .. } => Ok(content),
            other => panic!("expected ToolResult::File, got {other:?}"),
        }
    }

    /// Like `read`, but returns the full `ToolResult` so range/total-line
    /// metadata is inspectable, and takes an explicit line range.
    fn read_range(
        scope: &ToolScope,
        path: &str,
        start_line: Option<u32>,
        end_line: Option<u32>,
    ) -> Result<ToolResult, ToolError> {
        execute_tool(
            scope,
            ToolCall::ReadFile(ReadFileArgs {
                path: path.to_string(),
                start_line,
                end_line,
            }),
            &EmbeddingDeps::empty(),
        )
    }

    fn list(scope: &ToolScope, path: Option<&str>) -> Result<Vec<ToolFileEntry>, ToolError> {
        list_scoped(scope, path, None, None)
    }

    fn list_scoped(
        scope: &ToolScope,
        path: Option<&str>,
        depth: Option<u32>,
        pattern: Option<&str>,
    ) -> Result<Vec<ToolFileEntry>, ToolError> {
        match execute_tool(
            scope,
            ToolCall::ListFiles(ListFilesArgs {
                path: path.map(str::to_string),
                depth,
                pattern: pattern.map(str::to_string),
            }),
            &EmbeddingDeps::empty(),
        )? {
            ToolResult::FileList(entries) => Ok(entries),
            other => panic!("expected ToolResult::FileList, got {other:?}"),
        }
    }

    fn write(scope: &ToolScope, path: &str, content: &str) -> Result<String, ToolError> {
        match execute_tool(
            scope,
            ToolCall::WriteFile(WriteFileArgs {
                path: path.to_string(),
                content: content.to_string(),
            }),
            &EmbeddingDeps::empty(),
        )? {
            ToolResult::FileWritten { path } => Ok(path),
            other => panic!("expected ToolResult::FileWritten, got {other:?}"),
        }
    }

    fn edit(scope: &ToolScope, path: &str, edits: Vec<(&str, &str)>) -> Result<String, ToolError> {
        match execute_tool(
            scope,
            ToolCall::EditFile(EditFileArgs {
                path: path.to_string(),
                edits: edits
                    .into_iter()
                    .map(|(old, new)| FileEdit { old: old.to_string(), new: new.to_string() })
                    .collect(),
            }),
            &EmbeddingDeps::empty(),
        )? {
            ToolResult::FileEdited { path } => Ok(path),
            other => panic!("expected ToolResult::FileEdited, got {other:?}"),
        }
    }

    fn create_dir(scope: &ToolScope, path: &str) -> Result<String, ToolError> {
        match execute_tool(
            scope,
            ToolCall::CreateDirectory(CreateDirectoryArgs { path: path.to_string() }),
            &EmbeddingDeps::empty(),
        )? {
            ToolResult::DirectoryCreated { path } => Ok(path),
            other => panic!("expected ToolResult::DirectoryCreated, got {other:?}"),
        }
    }

    #[test]
    fn read_file_inside_docs_root_succeeds_in_docs_only() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let content = read(&scope, "intro.adoc").unwrap();
        assert_eq!(content, "= Intro\n");
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_on_a_directory_returns_not_a_file() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = read(&scope, ".").unwrap_err();
        assert!(matches!(err, ToolError::NotAFile(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_rejects_parent_escape_in_both_modes() {
        let (repo, docs) = fixture_repo();

        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let err = read(&docs_only, "../src/main.rs").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let err = read(&full_repo, "../outside.txt").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_same_relative_path_resolves_against_different_roots_by_mode() {
        let (repo, docs) = fixture_repo();

        // "src/main.rs" only exists under `repo`, not under `docs` — so the
        // same relative path is simply absent from the docs-only root
        // (there is no `docs/src/main.rs`), while it resolves fine once the
        // scope root widens to the whole repo. This is `ToolScope`'s mode
        // switch doing its job, not a `..`-escape (that's covered above).
        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        assert!(read(&docs_only, "src/main.rs").is_err());

        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let content = read(&full_repo, "src/main.rs").unwrap();
        assert_eq!(content, "fn main() {}\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn write_file_creates_and_overwrites_under_docs_root() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        write(&scope, "new.adoc", "= New\n").unwrap();
        assert_eq!(fs::read_to_string(docs.join("new.adoc")).unwrap(), "= New\n");

        write(&scope, "new.adoc", "= Replaced\n").unwrap();
        assert_eq!(fs::read_to_string(docs.join("new.adoc")).unwrap(), "= Replaced\n");

        fs::remove_dir_all(&repo).ok();
    }

    /// The load-bearing containment guarantee for `WriteFile`: even in
    /// `FullRepo` mode (where `ReadFile`/`ListFiles` resolve against
    /// `repo_root`), a write must land under `docs_root` — `FullRepo` widens
    /// read context, not write license.
    #[test]
    fn write_file_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        write(&full_repo, "guide.adoc", "= Guide\n").unwrap();

        assert_eq!(fs::read_to_string(docs.join("guide.adoc")).unwrap(), "= Guide\n");
        assert!(!repo.join("guide.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn write_file_rejects_unsupported_extension() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = write(&scope, "notes.py", "print('nope')\n").unwrap_err();
        assert!(matches!(err, ToolError::Io(_)));
        assert!(!docs.join("notes.py").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn write_file_rejects_path_escape() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = write(&scope, "../outside.adoc", "= Leak\n").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));
        assert!(!repo.join("outside.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_applies_a_single_replacement() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "private String name;\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        edit(
            &scope,
            "class.adoc",
            vec![("private String name;", "private String name;\nprivate int age;")],
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(docs.join("class.adoc")).unwrap(),
            "private String name;\nprivate int age;\n"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_applies_multiple_independent_edits_in_one_call() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\ngamma\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        edit(&scope, "class.adoc", vec![("alpha", "ALPHA"), ("gamma", "GAMMA")]).unwrap();
        assert_eq!(
            fs::read_to_string(docs.join("class.adoc")).unwrap(),
            "ALPHA\nbeta\nGAMMA\n"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_missing_file() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = edit(&scope, "does-not-exist.adoc", vec![("a", "b")]).unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_old_text_not_found_and_writes_nothing() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = edit(&scope, "class.adoc", vec![("nope", "NOPE")]).unwrap_err();
        assert!(matches!(err, ToolError::EditTextNotFound(_)));
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "alpha\nbeta\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_ambiguous_old_text_and_writes_nothing() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "dup\ndup\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = edit(&scope, "class.adoc", vec![("dup", "DUP")]).unwrap_err();
        assert!(matches!(err, ToolError::EditTextAmbiguous(_, 2)));
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "dup\ndup\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_overlapping_edits_and_writes_nothing() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "abcdef\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // "abcd" (0..4) and "cdef" (2..6) overlap on "cd".
        let err = edit(&scope, "class.adoc", vec![("abcd", "X"), ("cdef", "Y")]).unwrap_err();
        assert!(matches!(err, ToolError::EditsOverlap));
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "abcdef\n");

        fs::remove_dir_all(&repo).ok();
    }

    /// Edits are validated against the file's *original* content, not
    /// sequentially against each other's output — an edit whose `new` text
    /// happens to equal another edit's `old` text must not make that other
    /// edit's match count look any different.
    #[test]
    fn edit_file_validates_edits_against_original_content_not_sequentially() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // If edits were applied sequentially, "alpha" -> "beta" would make
        // "beta" ambiguous (two occurrences) by the time the second edit's
        // match is checked. Validated against the original, "beta" still
        // matches exactly once.
        edit(&scope, "class.adoc", vec![("alpha", "beta"), ("beta", "BETA")]).unwrap();
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "beta\nBETA\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("guide.adoc"), "old text\n").unwrap();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        edit(&full_repo, "guide.adoc", vec![("old text", "new text")]).unwrap();
        assert_eq!(fs::read_to_string(docs.join("guide.adoc")).unwrap(), "new text\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_path_escape() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = edit(&scope, "../outside.adoc", vec![("a", "b")]).unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn create_directory_creates_missing_parents_under_docs_root() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        create_dir(&scope, "guides/nested").unwrap();

        assert!(docs.join("guides/nested").is_dir());

        fs::remove_dir_all(&repo).ok();
    }

    /// Same containment guarantee as `write_file_full_repo_mode_still_targets_docs_root_not_repo_root`:
    /// even in `FullRepo` mode a new directory must land under `docs_root`.
    #[test]
    fn create_directory_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        create_dir(&full_repo, "endpoints").unwrap();

        assert!(docs.join("endpoints").is_dir());
        assert!(!repo.join("endpoints").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn create_directory_rejects_an_already_existing_path() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        create_dir(&scope, "guides").unwrap();
        let err = create_dir(&scope, "guides").unwrap_err();
        assert!(matches!(err, ToolError::Io(_)));

        // Also rejected when the path is already occupied by a file.
        let err = create_dir(&scope, "intro.adoc").unwrap_err();
        assert!(matches!(err, ToolError::Io(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn create_directory_rejects_path_escape() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // Unlike `write_project_file`, `create_project_dir` runs
        // `validate_relative_name` first, which rejects a `..` component
        // itself (`ProjectError::InvalidName`) before `join_relative` ever
        // gets a chance to produce `PathEscape` — still safely rejected,
        // just via the generic `Io` catch-all in the `ProjectError` ->
        // `ToolError` mapping, since `InvalidName` has no dedicated arm.
        let err = create_dir(&scope, "../outside-dir").unwrap_err();
        assert!(matches!(err, ToolError::Io(_)));
        assert!(!repo.join("outside-dir").exists());

        fs::remove_dir_all(&repo).ok();
    }

    /// `join_relative`'s `..`-rejection only catches lexical traversal; the
    /// real defense against a path that resolves outside the root by other
    /// means (e.g. a symlink) is `ensure_under`'s canonicalize+`starts_with`
    /// check. Exercise that directly so the containment guarantee isn't
    /// only proven for the lexical case.
    #[cfg(unix)]
    #[test]
    fn read_file_rejects_symlink_escaping_docs_root() {
        let (repo, docs) = fixture_repo();
        std::os::unix::fs::symlink(repo.join("src/main.rs"), docs.join("leak.adoc")).unwrap();

        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let err = read(&docs_only, "leak.adoc").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_docs_only_excludes_source_files() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let entries = list(&scope, None).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"intro.adoc"));
        assert!(!paths.contains(&"script.py"));
        assert!(!paths.iter().any(|p| p.ends_with("main.rs")));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_full_repo_includes_source_files() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let entries = list(&scope, None).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"docs/intro.adoc"));
        assert!(paths.contains(&"docs/script.py"));
        assert!(paths.contains(&"src/main.rs"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_full_read_reports_total_lines_and_is_byte_identical() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "intro.adoc", None, None).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "= Intro\n");
                assert_eq!((start_line, end_line, total_lines), (1, 1, 1));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_returns_a_requested_line_range() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("multi.adoc"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "multi.adoc", Some(2), Some(4)).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "two\nthree\nfour\n");
                assert_eq!((start_line, end_line, total_lines), (2, 4, 5));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_clamps_out_of_range_start_and_end_line() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("multi.adoc"), "one\ntwo\nthree\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "multi.adoc", Some(0), Some(9999)).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "one\ntwo\nthree\n");
                assert_eq!((start_line, end_line, total_lines), (1, 3, 3));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_clamps_end_line_below_start_line_up_to_start_line() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("multi.adoc"), "one\ntwo\nthree\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "multi.adoc", Some(3), Some(1)).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "three\n");
                assert_eq!((start_line, end_line, total_lines), (3, 3, 3));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_line_range_on_empty_file_reports_zero_lines() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("empty.adoc"), "").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "empty.adoc", Some(1), Some(5)).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "");
                assert_eq!((start_line, end_line, total_lines), (0, 0, 0));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    /// The key regression test for the `list_docs_only` walk-scoping fix:
    /// without it, `depth` would be measured from `docs_root` instead of
    /// from the requested `path`, silently producing wrong results.
    #[test]
    fn list_files_depth_is_relative_to_requested_subdir_not_root() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("a/b")).unwrap();
        fs::write(docs.join("a/direct.adoc"), "= Direct\n").unwrap();
        fs::write(docs.join("a/b/nested.adoc"), "= Nested\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let entries = list_scoped(&scope, Some("a"), Some(1), None).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"a/direct.adoc"));
        assert!(paths.contains(&"a/b"));
        // depth=1 relative to "a" excludes "a"'s grandchildren.
        assert!(!paths.contains(&"a/b/nested.adoc"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_depth_limits_recursion_in_docs_only() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("a/b")).unwrap();
        fs::write(docs.join("a/one.adoc"), "= One\n").unwrap();
        fs::write(docs.join("a/b/two.adoc"), "= Two\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let shallow = list_scoped(&scope, None, Some(2), None).unwrap();
        let shallow_paths: Vec<&str> = shallow.iter().map(|e| e.path.as_str()).collect();
        assert!(shallow_paths.contains(&"a/one.adoc"));
        assert!(!shallow_paths.contains(&"a/b/two.adoc"));

        let unlimited = list_scoped(&scope, None, None, None).unwrap();
        let unlimited_paths: Vec<&str> = unlimited.iter().map(|e| e.path.as_str()).collect();
        assert!(unlimited_paths.contains(&"a/b/two.adoc"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_depth_limits_recursion_in_full_repo() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(repo.join("src/nested")).unwrap();
        fs::write(repo.join("src/nested/deep.rs"), "fn deep() {}\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let shallow = list_scoped(&scope, None, Some(2), None).unwrap();
        let shallow_paths: Vec<&str> = shallow.iter().map(|e| e.path.as_str()).collect();
        assert!(shallow_paths.contains(&"src/main.rs"));
        assert!(!shallow_paths.iter().any(|p| p.ends_with("deep.rs")));

        let unlimited = list_scoped(&scope, None, None, None).unwrap();
        let unlimited_paths: Vec<&str> = unlimited.iter().map(|e| e.path.as_str()).collect();
        assert!(unlimited_paths.iter().any(|p| p.ends_with("deep.rs")));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_depth_zero_returns_no_descendant_entries() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let entries = list_scoped(&scope, None, Some(0), None).unwrap();
        assert!(entries.is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_pattern_filters_by_basename_across_depths() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(repo.join("src/sub")).unwrap();
        fs::write(repo.join("src/a.java"), "class A {}\n").unwrap();
        fs::write(repo.join("src/sub/b.java"), "class B {}\n").unwrap();
        fs::write(repo.join("src/sub/c.txt"), "not java\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let entries = list_scoped(&scope, Some("src"), None, Some("*.java")).unwrap();
        let mut paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, vec!["src/a.java", "src/sub/b.java"]);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_pattern_keeps_directory_entries() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("assets")).unwrap();
        fs::write(docs.join("assets/logo.png"), "not adoc").ok();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // `assets` itself doesn't match "*.adoc", but must still be listed
        // — `pattern` scopes which files come back, not the directory
        // structure.
        let entries = list_scoped(&scope, None, None, Some("*.adoc")).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"assets"));
        assert!(paths.contains(&"intro.adoc"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_invalid_glob_pattern_returns_invalid_pattern_error() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = list_scoped(&scope, None, None, Some("[")).unwrap_err();
        assert!(matches!(err, ToolError::InvalidPattern(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn execute_tool_denies_a_tool_missing_from_a_customized_allowlist() {
        let (repo, docs) = fixture_repo();
        let only_list: HashSet<ToolName> = [ToolName::ListFiles].into_iter().collect();
        let scope = ToolScope::new(&repo, &docs, AiAccessMode::DocsOnly, only_list);

        let err = read(&scope, "intro.adoc").unwrap_err();
        assert!(matches!(err, ToolError::NotAllowed(ToolName::ReadFile)));

        // The other tool in the same customized allowlist still works.
        assert!(list(&scope, None).is_ok());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn execute_tool_denies_semantic_search_missing_from_a_customized_allowlist() {
        let (repo, docs) = fixture_repo();
        let only_list: HashSet<ToolName> =
            [ToolName::ListFiles, ToolName::ReadFile].into_iter().collect();
        let scope = ToolScope::new(&repo, &docs, AiAccessMode::DocsOnly, only_list);

        let err = execute_tool(
            &scope,
            ToolCall::SemanticSearch(SemanticSearchArgs {
                query: "intro".to_string(),
                top_k: None,
            }),
            &EmbeddingDeps::empty(),
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::NotAllowed(ToolName::SemanticSearch)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn scope_for_config_defaults_to_both_tools_when_unset() {
        let (repo, docs) = fixture_repo();
        let config = ProjectConfig::new(".");

        let scope = scope_for_config(&repo, &docs, &config);
        assert!(read(&scope, "intro.adoc").is_ok());
        assert!(list(&scope, None).is_ok());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn scope_for_config_honors_a_customized_allowlist() {
        let (repo, docs) = fixture_repo();
        let mut config = ProjectConfig::new(".");
        config.ai_allowed_tools = Some(vec![ToolName::ListFiles]);

        let scope = scope_for_config(&repo, &docs, &config);
        assert!(matches!(
            read(&scope, "intro.adoc").unwrap_err(),
            ToolError::NotAllowed(ToolName::ReadFile)
        ));
        assert!(list(&scope, None).is_ok());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn tool_call_and_result_round_trip_through_json() {
        let call = ToolCall::ReadFile(ReadFileArgs {
            path: "intro.adoc".to_string(),
            start_line: None,
            end_line: None,
        });
        let json = serde_json::to_string(&call).unwrap();
        assert_eq!(
            json,
            r#"{"tool":"readFile","args":{"path":"intro.adoc","startLine":null,"endLine":null}}"#
        );
        let round_tripped: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, call);

        let result = ToolResult::File {
            content: "= Intro\n".to_string(),
            start_line: 1,
            end_line: 1,
            total_lines: 1,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert_eq!(
            json,
            r#"{"tool":"file","result":{"content":"= Intro\n","startLine":1,"endLine":1,"totalLines":1}}"#
        );
    }

    #[test]
    fn symbol_matches_finds_an_exact_case_insensitive_name() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/UserService.java"),
            "public class UserService {\n    public String getName() { return null; }\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(&repo_index, &scope, "userservice", 10);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].source, MatchSource::Symbol);
        assert!(matches[0].path.ends_with("UserService.java"));
        assert_eq!(matches[0].score, 1.0);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_is_empty_for_an_unknown_name() {
        let (repo, docs) = fixture_repo();
        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        assert!(symbol_matches(&repo_index, &scope, "NoSuchSymbol", 10).is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_excludes_non_doc_symbols_in_docs_only() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/UserService.java"),
            "public class UserService {\n    public String getName() { return null; }\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        assert!(symbol_matches(&repo_index, &scope, "userservice", 10).is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_finds_a_case_insensitive_substring() {
        use crate::domain::chunk_index::ChunkBuildOptions;
        use crate::services::chunk_builder::ChunkBuilder;

        let (repo, docs) = fixture_repo();
        fs::write(repo.join("docs/needle.adoc"), "= Guide\n\nfind the NEEDLE here\n").unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(ChunkBuilder::new().build_all(&repo_index, &ChunkBuildOptions::default()));
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = lexical_matches(&chunk_index, &scope, "needle", 10);
        assert!(!matches.is_empty());
        assert_eq!(matches[0].source, MatchSource::Lexical);
        assert!(matches[0].snippet.to_lowercase().contains("needle"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_is_empty_for_an_empty_query() {
        let (repo, docs) = fixture_repo();
        let chunk_index = ChunkIndex::new();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        assert!(lexical_matches(&chunk_index, &scope, "", 10).is_empty());
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_excludes_non_doc_chunks_in_docs_only() {
        use crate::domain::chunk_index::ChunkBuildOptions;
        use crate::services::chunk_builder::ChunkBuilder;

        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/Needle.java"),
            "public class Needle {\n    // find the NEEDLE here\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(ChunkBuilder::new().build_all(&repo_index, &ChunkBuildOptions::default()));
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        assert!(lexical_matches(&chunk_index, &scope, "needle", 10).is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn parse_tool_call_parses_read_file_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "readFile".to_string(),
            arguments: r#"{"path":"intro.adoc"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(parsed, ToolCall::ReadFile(ReadFileArgs { path: "intro.adoc".to_string(), start_line: None, end_line: None }));
    }

    #[test]
    fn parse_tool_call_parses_list_files_args_with_null_path() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "listFiles".to_string(),
            arguments: r#"{"path":null}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(parsed, ToolCall::ListFiles(ListFilesArgs { path: None, depth: None, pattern: None }));
    }

    #[test]
    fn parse_tool_call_parses_read_file_args_with_line_range() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "readFile".to_string(),
            arguments: r#"{"path":"intro.adoc","startLine":2,"endLine":10}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::ReadFile(ReadFileArgs {
                path: "intro.adoc".to_string(),
                start_line: Some(2),
                end_line: Some(10),
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_list_files_args_with_depth_and_pattern() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "listFiles".to_string(),
            arguments: r#"{"path":"src","depth":2,"pattern":"*.java"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::ListFiles(ListFilesArgs {
                path: Some("src".to_string()),
                depth: Some(2),
                pattern: Some("*.java".to_string()),
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_semantic_search_args_with_top_k() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "semanticSearch".to_string(),
            arguments: r#"{"query":"auth flow","topK":5}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::SemanticSearch(SemanticSearchArgs { query: "auth flow".to_string(), top_k: Some(5) })
        );
    }

    #[test]
    fn parse_tool_call_parses_write_file_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "writeFile".to_string(),
            arguments: r#"{"path":"guide.adoc","content":"= Guide\n"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::WriteFile(WriteFileArgs {
                path: "guide.adoc".to_string(),
                content: "= Guide\n".to_string(),
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_create_directory_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "createDirectory".to_string(),
            arguments: r#"{"path":"guides/nested"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::CreateDirectory(CreateDirectoryArgs { path: "guides/nested".to_string() })
        );
    }

    #[test]
    fn parse_tool_call_parses_edit_file_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "editFile".to_string(),
            arguments: r#"{"path":"guide.adoc","edits":[{"old":"a","new":"b"}]}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::EditFile(EditFileArgs {
                path: "guide.adoc".to_string(),
                edits: vec![FileEdit { old: "a".to_string(), new: "b".to_string() }],
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_request_full_repo_access_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "requestFullRepoAccess".to_string(),
            arguments: r#"{"reason":"need to check the config schema"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::RequestFullRepoAccess(RequestFullRepoAccessArgs {
                reason: "need to check the config schema".to_string(),
            })
        );
    }

    #[test]
    fn parse_tool_call_rejects_unknown_tool_name() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "deleteFile".to_string(),
            arguments: "{}".to_string(),
        };
        let err = parse_tool_call(&call).unwrap_err();
        assert!(matches!(err, ToolError::UnknownTool(name) if name == "deleteFile"));
    }

    #[test]
    fn parse_tool_call_rejects_malformed_arguments_json() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "readFile".to_string(),
            arguments: "{not json}".to_string(),
        };
        let err = parse_tool_call(&call).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { tool, .. } if tool == "readFile"));
    }

    #[test]
    fn parse_tool_call_tolerates_trailing_garbage_after_a_complete_object() {
        // Real observed case: a provider streamed `listFiles` arguments as
        // fragments that concatenated into `{}""` — a complete, valid empty
        // object followed by a stray extra fragment. Strict `serde_json`
        // rejects this ("trailing characters"); the lenient fallback should
        // still recover the real (empty) arguments rather than surfacing an
        // error the user can do nothing about.
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "listFiles".to_string(),
            arguments: "{}\"\"".to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(parsed, ToolCall::ListFiles(ListFilesArgs { path: None, depth: None, pattern: None }));
    }

    #[test]
    fn parse_tool_call_tolerates_trailing_garbage_after_a_populated_object() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "readFile".to_string(),
            arguments: r#"{"path":"intro.adoc"}garbage"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(parsed, ToolCall::ReadFile(ReadFileArgs { path: "intro.adoc".to_string(), start_line: None, end_line: None }));
    }

    #[test]
    fn parse_tool_call_still_rejects_arguments_that_are_invalid_from_the_start() {
        // The lenient fallback only rescues "valid value + trailing junk" —
        // JSON that's broken from the very first token must still error,
        // so the model still gets an honest, actionable error to learn
        // from rather than silently defaulting to empty arguments.
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "readFile".to_string(),
            arguments: "{not json at all".to_string(),
        };
        let err = parse_tool_call(&call).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { tool, .. } if tool == "readFile"));
    }

    #[test]
    fn llm_tool_definitions_includes_all_seven_by_default() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let defs = llm_tool_definitions(&scope);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "listFiles",
                "readFile",
                "semanticSearch",
                "writeFile",
                "editFile",
                "createDirectory",
                "requestFullRepoAccess"
            ]
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn llm_tool_definitions_excludes_a_tool_missing_from_a_customized_allowlist() {
        let (repo, docs) = fixture_repo();
        let only_list: HashSet<ToolName> = [ToolName::ListFiles].into_iter().collect();
        let scope = ToolScope::new(&repo, &docs, AiAccessMode::DocsOnly, only_list);

        let defs = llm_tool_definitions(&scope);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["listFiles"]);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn llm_tool_definitions_parameters_round_trip_a_realistic_arguments_payload() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let defs = llm_tool_definitions(&scope);

        let read_file = defs.iter().find(|d| d.name == "readFile").unwrap();
        assert_eq!(read_file.parameters["required"], serde_json::json!(["path"]));
        let args: ReadFileArgs =
            serde_json::from_value(serde_json::json!({"path": "intro.adoc"})).unwrap();
        assert_eq!(args.path, "intro.adoc");
        assert_eq!(args.start_line, None);
        assert_eq!(args.end_line, None);
        let args: ReadFileArgs = serde_json::from_value(
            serde_json::json!({"path": "intro.adoc", "startLine": 2, "endLine": 10}),
        )
        .unwrap();
        assert_eq!(args.start_line, Some(2));
        assert_eq!(args.end_line, Some(10));

        let list_files = defs.iter().find(|d| d.name == "listFiles").unwrap();
        assert_eq!(list_files.parameters["required"], serde_json::json!([]));
        let args: ListFilesArgs = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(args.path, None);
        assert_eq!(args.depth, None);
        assert_eq!(args.pattern, None);
        let args: ListFilesArgs = serde_json::from_value(
            serde_json::json!({"path": "src", "depth": 2, "pattern": "*.java"}),
        )
        .unwrap();
        assert_eq!(args.depth, Some(2));
        assert_eq!(args.pattern, Some("*.java".to_string()));

        let semantic_search = defs.iter().find(|d| d.name == "semanticSearch").unwrap();
        assert_eq!(semantic_search.parameters["required"], serde_json::json!(["query"]));
        let args: SemanticSearchArgs =
            serde_json::from_value(serde_json::json!({"query": "x", "topK": 3})).unwrap();
        assert_eq!(args.query, "x");
        assert_eq!(args.top_k, Some(3));

        fs::remove_dir_all(&repo).ok();
    }
}
