use std::sync::Arc;

use tauri::State;

use crate::commands::embeddings::{
    EmbeddingIndexSlot, EmbeddingProviderSlot, EmbeddingSyncGuard, IndexStoreSlot,
};
use crate::domain::ai_access::AiAccessMode;
use crate::domain::ai_tools::{ToolCall, ToolResult};
use crate::domain::llm::LlmToolDefinition;
use crate::domain::project_config::ProjectConfig;
use crate::infra::project_store;
use crate::services::ai_tools;
use crate::services::ai_tools::EmbeddingDeps;
use crate::services::chunk_builder::ChunkIndex;
use crate::services::project_open;
use crate::services::repo_index::RepositoryIndex;
use crate::services::workspace_index::WorkspaceIndex;

/// The frontend passes only `{ tool, args }` — no `docsRoot`/`repoRoot`, no
/// access mode. `services::ai_tools::current_scope()` resolves whichever
/// project is currently open and its persisted allowlist/mode itself. The
/// seven embedding/chunk/repo-index/workspace-index `State` params are what
/// `SemanticSearch`/`Move` need (see `services::ai_tools::EmbeddingDeps`) —
/// all already managed globally in `lib.rs`, cloned here into one bundle
/// exactly like `commands::embeddings::embedding_sync` already does for its
/// own params. `spawn_blocking` because `ListFiles` in `FullRepo` mode walks
/// the whole repo (`infra::workspace_scanner::scan_all`), and
/// `SemanticSearch` may run model inference — both comparable in cost to
/// `check_standards`'s repo-wide walk.
#[tauri::command]
pub async fn ai_execute_tool(
    call: ToolCall,
    repo_index: State<'_, Arc<RepositoryIndex>>,
    chunk_index: State<'_, Arc<ChunkIndex>>,
    embedding_index: State<'_, Arc<EmbeddingIndexSlot>>,
    index_store: State<'_, Arc<IndexStoreSlot>>,
    embedding_provider: State<'_, Arc<EmbeddingProviderSlot>>,
    sync_guard: State<'_, Arc<EmbeddingSyncGuard>>,
    workspace_index: State<'_, Arc<WorkspaceIndex>>,
) -> Result<ToolResult, String> {
    let deps = EmbeddingDeps {
        repo_index: repo_index.inner().clone(),
        chunk_index: chunk_index.inner().clone(),
        embedding_index: embedding_index.inner().clone(),
        index_store: index_store.inner().clone(),
        embedding_provider: embedding_provider.inner().clone(),
        sync_guard: sync_guard.inner().clone(),
        workspace_index: workspace_index.inner().clone(),
        // No chat turn here to reuse a resolved provider/model from — this
        // standalone endpoint has no fast-apply fallback for `EditFile`, so
        // a non-exact edit surfaces the plain deterministic error. See
        // `EmbeddingDeps::fast_apply`'s doc comment.
        fast_apply: None,
    };
    tauri::async_runtime::spawn_blocking(move || {
        let scope = ai_tools::current_scope().map_err(|e| e.to_string())?;
        ai_tools::execute_tool(&scope, call, &deps).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Loads the currently open project's persisted `AiAccessMode` — the same
/// boundary `ai_execute_tool` and `embedding_sync` both read via
/// `ProjectConfig`. Missing `project.json` → the safe `DocsOnly` default,
/// same fallback `ProjectConfig::new` uses everywhere else.
#[tauri::command]
pub fn ai_get_access_mode() -> Result<AiAccessMode, String> {
    let project = project_open::get_project()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no project is open".to_string())?;
    let config = project_store::load(&project.root)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| ProjectConfig::new(project.docs_root.clone()));
    Ok(config.ai_access_mode)
}

/// The same tool definitions (`name`/`description`/JSON-Schema `parameters`)
/// actually advertised to the model for function-calling in
/// `commands::llm::llm_chat_stream` — exposed here so the frontend can
/// render its "available tools" prompt text from the same source instead of
/// hand-duplicating it.
#[tauri::command]
pub fn ai_get_tool_definitions() -> Result<Vec<LlmToolDefinition>, String> {
    let scope = ai_tools::current_scope().map_err(|e| e.to_string())?;
    Ok(ai_tools::llm_tool_definitions(&scope))
}

/// Persists a new `AiAccessMode` for the currently open project — thin
/// wrapper over `services::ai_tools::set_access_mode`, shared with the
/// `RequestFullRepoAccess` tool so a mode change behaves identically
/// regardless of which path triggered it (preserves any existing
/// `ai_allowed_tools` override rather than resetting it).
#[tauri::command]
pub fn ai_set_access_mode(mode: AiAccessMode) -> Result<(), String> {
    ai_tools::set_access_mode(mode).map_err(|e| e.to_string())
}
