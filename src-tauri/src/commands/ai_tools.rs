use std::sync::Arc;

use tauri::State;

use crate::commands::embeddings::{
    EmbeddingIndexSlot, EmbeddingProviderSlot, EmbeddingSyncGuard, IndexStoreSlot,
};
use crate::domain::ai_access::{AiAccessMode, ToolName};
use crate::domain::ai_tools::{Task, ToolCall, ToolResult};
use crate::domain::conversation_mode::ConversationMode;
use crate::domain::llm::LlmToolDefinition;
use crate::domain::project_config::ProjectConfig;
use crate::infra::project_store;
use crate::services::ai_tools;
use crate::services::ai_tools::{EmbeddingDeps, ToolCallLogContext};
use crate::services::llm_config;
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
///
/// `todos` has no persisted meaning here — this endpoint is stateless
/// between calls, unlike the chat loop's `todoListRef`; callers invoking
/// the `todo` tool through this path must track and resupply the list
/// themselves (typically empty for any other tool call).
#[tauri::command]
pub async fn ai_execute_tool(
    call: ToolCall,
    todos: Vec<Task>,
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
        // Same standalone-endpoint reasoning as `fast_apply` above — no
        // editor tab context reaches this call. See `EmbeddingDeps::
        // active_file`'s doc comment.
        active_file: None,
    };
    tauri::async_runtime::spawn_blocking(move || {
        let scope = ai_tools::current_scope().map_err(|e| e.to_string())?;
        // Best-effort — an unreadable settings file must not block the tool
        // call itself, only silently disable its log entry.
        let enabled = llm_config::load_llm_settings().map(|s| s.tool_call_logging).unwrap_or(false);
        let log_ctx = ToolCallLogContext { enabled, source: "standalone", round: None, provider_id: None, model: None };
        ai_tools::execute_tool_logged(&scope, call, &deps, &todos, &log_ctx).map_err(|e| e.to_string())
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
/// hand-duplicating it. `conversation_mode` must match whatever the caller
/// intends to actually chat in (see `domain::conversation_mode`) — this
/// endpoint has no chat turn of its own to infer it from.
#[tauri::command]
pub fn ai_get_tool_definitions(
    conversation_mode: ConversationMode,
) -> Result<Vec<LlmToolDefinition>, String> {
    let scope = ai_tools::current_scope().map_err(|e| e.to_string())?;
    Ok(ai_tools::llm_tool_definitions(&scope, conversation_mode))
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

/// Tool names the currently open project has persisted as "always allow" —
/// loaded once by the frontend when an assistant chat panel mounts, to seed
/// its in-memory trusted-tool set so a choice made in one chat carries into
/// every later chat on this repo.
#[tauri::command]
pub fn ai_get_auto_approved_tools() -> Result<Vec<ToolName>, String> {
    Ok(ai_tools::auto_approved_tools()
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect())
}

/// Persists (or revokes) one tool's "always allow" status for the currently
/// open project — called when the user clicks "Разрешать всегда" (or, in
/// the future, revokes it) on an approval card.
#[tauri::command]
pub fn ai_set_tool_auto_approved(tool: ToolName, auto_approved: bool) -> Result<(), String> {
    ai_tools::set_tool_auto_approved(tool, auto_approved).map_err(|e| e.to_string())
}

/// Tool names the currently open project actually allows right now — the
/// customized `ai_allowed_tools` set if one was ever saved, else `mode`'s
/// default. Backs the Settings "Разрешённые инструменты" list.
#[tauri::command]
pub fn ai_get_allowed_tools() -> Result<Vec<ToolName>, String> {
    Ok(ai_tools::allowed_tools()
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect())
}

/// Persists (or revokes) one tool's membership in `ai_allowed_tools` for the
/// currently open project — called from the Settings "Разрешённые
/// инструменты" checkbox list.
#[tauri::command]
pub fn ai_set_tool_allowed(tool: ToolName, allowed: bool) -> Result<(), String> {
    ai_tools::set_tool_allowed(tool, allowed).map_err(|e| e.to_string())
}

/// Combined OptMem wake for project + global stores — injected into the
/// chat system context at the start of a turn so the model does not have
/// to remember to call `memory`/`wake` first. Returns an empty string when
/// the `memory` tool is disallowed for the open project, or both stores are
/// empty/missing.
#[tauri::command]
pub fn ai_get_memory_wake() -> Result<String, String> {
    let project = project_open::get_project().map_err(|e| e.to_string())?;
    let Some(project) = project else {
        return Err("no project open".to_string());
    };
    let allowed = crate::services::ai_tools::allowed_tools().map_err(|e| e.to_string())?;
    if !allowed.contains(&crate::domain::ai_access::ToolName::Memory) {
        return Ok(String::new());
    }
    let root = std::path::PathBuf::from(&project.root);
    crate::services::agent_memory::wake_context(&root).map_err(|e| e.to_string())
}
