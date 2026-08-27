//! The tools the AI harness can call, and the machinery around them.
//!
//! Layout: [`tools`] holds one module per tool, each carrying both the JSON
//! schema the model is shown and the implementation behind it. The rest is
//! shared — [`parse`] turns the model's raw JSON into a typed `ToolCall`,
//! [`resolve`] turns the paths inside it into real paths on disk, [`scope`]
//! owns the persisted access mode and allowlist, and [`search`] is the
//! matching engine behind `semanticSearch`.
//!
//! [`resolve`] is the enforcement point for `AiAccessMode`: every tool that
//! takes a path goes through it, and containment is resolved against
//! `scope.root` via `domain::paths` — the same primitives
//! `services::docs_fs` uses. A caller can never widen access by passing an
//! unexpected path, only by the `ToolScope` itself having been built with a
//! wider root.
//!
//! What is left here is the state the tools reach for (`EmbeddingDeps`) and
//! the audit-logging wrapper around `execute_tool`.

pub mod parse;
pub mod resolve;
pub mod scope;
pub mod search;
pub mod tools;
#[cfg(test)]
pub(crate) mod testing;

use std::sync::Arc;

use crate::domain::ai_tools::{Task, ToolCall, ToolError, ToolResult, ToolScope};
use crate::domain::llm::LlmProvider;
use crate::domain::repo_index::FileId;
use crate::services::chunk_builder::ChunkIndex;
use crate::services::embedding_state::{
    EmbeddingIndexSlot, EmbeddingProviderSlot, EmbeddingSyncGuard, IndexStoreSlot,
};
use crate::services::repo_index::RepositoryIndex;
use crate::services::workspace_index::WorkspaceIndex;

pub use parse::{parse_tool_call, preflight_tool_call};
pub use scope::{
    allowed_tools, auto_approved_tools, current_scope, set_access_mode, set_tool_allowed,
    set_tool_auto_approved,
};
pub use tools::{execute_tool, llm_tool_definitions, render_file_tree};

/// The embedding/chunk/repo-index/workspace-index state `SemanticSearch`/
/// `Move` need to reach — `execute_tool` is otherwise a pure function with
/// no access to Tauri-managed state. Mirrors exactly what
/// `commands::embeddings::embedding_sync` already receives as
/// `State<'_, Arc<T>>` params; `commands::ai_tools::ai_execute_tool`/
/// `commands::llm::llm_chat_stream`/`llm_chat_stream_resume` each clone
/// their own `State`s into this struct before calling `execute_tool`.
/// `workspace_index` was added for `move_path`'s reference-rewrite step
/// (`services::reference_rewrite::rewrite_references`) — the name predates
/// that and is now a bit imprecise, but this is the one established
/// extension point for a new tool needing more Tauri-managed state, not
/// worth a second threading mechanism for one field.
pub struct EmbeddingDeps {
    pub repo_index: Arc<RepositoryIndex>,
    pub chunk_index: Arc<ChunkIndex>,
    pub embedding_index: Arc<EmbeddingIndexSlot>,
    pub index_store: Arc<IndexStoreSlot>,
    pub embedding_provider: Arc<EmbeddingProviderSlot>,
    pub sync_guard: Arc<EmbeddingSyncGuard>,
    pub workspace_index: Arc<WorkspaceIndex>,
    /// `EditFile`'s fast-apply fallback capability — the `(provider, model)`
    /// pair already resolved for the surrounding chat turn, reused rather
    /// than resolving a second one just for this. `None` disables the
    /// fallback entirely (`edit_file` then behaves exactly as it always
    /// did: a plain deterministic `EditTextNotFound`/`EditTextAmbiguous` on
    /// a non-exact match). `services::llm_chat::run_tool_loop` is the one caller
    /// that sets this; `commands::ai_tools::ai_execute_tool` (a standalone
    /// endpoint with no chat turn to reuse a resolved provider from) leaves
    /// it `None`. Reuses this struct rather than adding a second
    /// threading mechanism for one field — see this struct's own doc
    /// comment above, which already establishes that precedent for `Move`.
    pub fast_apply: Option<(Arc<dyn LlmProvider>, String)>,
    /// The user's currently-open editor tab, if any — `FileId`-space
    /// (already converted from the frontend's docs-root-relative
    /// `EditorTab.path` by `services::llm_chat::setup`, the same conversion
    /// `embedding_set_priority_files` already establishes as precedent).
    /// `semantic_search` uses this to boost chunks from files related to it
    /// (via `related_files`) — `None` disables the boost entirely, same as
    /// today's unboosted ranking. `commands::ai_tools::ai_execute_tool` (no
    /// chat turn, no editor context) leaves it `None`, same as `fast_apply`.
    pub active_file: Option<FileId>,
}

#[cfg(test)]
impl EmbeddingDeps {
    /// Fresh, empty instances of every slot — for `ReadFile`/`ListFiles`
    /// tests (which never touch these) and as a base for `SemanticSearch`/
    /// `Move` tests that need to populate specific state.
    pub fn empty() -> Self {
        Self {
            repo_index: Arc::new(RepositoryIndex::new()),
            chunk_index: Arc::new(ChunkIndex::new()),
            embedding_index: Arc::new(EmbeddingIndexSlot::new(None)),
            index_store: Arc::new(IndexStoreSlot::new(None)),
            embedding_provider: Arc::new(EmbeddingProviderSlot::new(None)),
            sync_guard: Arc::new(EmbeddingSyncGuard::new(())),
            workspace_index: Arc::new(WorkspaceIndex::new(
                crate::infra::parsers::registry::ParserRegistry::new(),
            )),
            fast_apply: None,
            active_file: None,
        }
    }
}

/// Correlation info available at a real (non-test) call site, threaded
/// through to the persisted log row — see `domain::tool_call_log::
/// ToolCallLogRow` for what each field means. Built fresh by each caller
/// from whatever it already has on hand (`commands::ai_tools::
/// ai_execute_tool` has no chat turn to draw `round`/`provider_id`/`model`
/// from; `services::llm_chat::run_tool_loop` does).
pub struct ToolCallLogContext {
    pub enabled: bool,
    pub source: &'static str,
    pub round: Option<u32>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
}

/// `execute_tool`, plus a redacted entry written to the persisted tool-call
/// log (`infra::tool_call_log`) once the call settles. Deliberately a thin
/// wrapper rather than logging inside `execute_tool` itself: `execute_tool`
/// has ~30 direct call sites in this module's own test suite, all of which
/// exercise it as the pure, I/O-free function it already is — folding
/// logging into it would force every one of those tests to also touch a
/// SQLite file. The two real callers (`commands::ai_tools::ai_execute_tool`,
/// `services::llm_chat::run_tool_loop`) call this instead.
pub fn execute_tool_logged(
    scope: &ToolScope,
    call: ToolCall,
    deps: &EmbeddingDeps,
    todos: &[Task],
    log_ctx: &ToolCallLogContext,
) -> Result<ToolResult, ToolError> {
    let tool = crate::infra::tool_call_log::tool_label(&call);
    let args_json = crate::infra::tool_call_log::redact_args(&call);
    let started = std::time::Instant::now();
    let result = execute_tool(scope, call, deps, todos);
    let duration_ms = started.elapsed().as_millis() as i64;

    crate::infra::tool_call_log::log_call(
        log_ctx.enabled,
        crate::infra::tool_call_log::ToolCallLogEntry {
            repo_root: scope.repo_root.display().to_string(),
            source: log_ctx.source.to_string(),
            round: log_ctx.round,
            provider_id: log_ctx.provider_id.clone(),
            model: log_ctx.model.clone(),
            tool,
            args_json,
            status: if result.is_ok() { "ok" } else { "error" }.to_string(),
            error_message: result.as_ref().err().map(ToString::to_string),
            result_json: result
                .as_ref()
                .ok()
                .map(crate::infra::tool_call_log::redact_result),
            duration_ms,
        },
    );

    result
}

