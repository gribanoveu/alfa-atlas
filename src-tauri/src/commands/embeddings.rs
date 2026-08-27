use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::domain::embeddings::{
    EmbeddingIndexStatus, EmbeddingProviderConfig, ModelDownloadProgress, ModelDownloadSink,
    ModelStatus, ResolvedEmbeddingConfig, SyncProgress, SyncStats,
};
use crate::domain::paths;
use crate::domain::repo_index::FileId;
use crate::infra::embedding_credentials_store;
use crate::services::chunk_builder::ChunkIndex;
use crate::services::embedding_config;
use crate::services::embedding_model::{self, DownloadState};
use crate::services::embedding_sync::{self, ProgressSink};
use crate::services::embedding_state::{
    resolve_index_paths, BackgroundBacklogSlot, EmbeddingIndexSlot, EmbeddingProviderSlot,
    EmbeddingSession, EmbeddingSyncGuard, FullSyncActiveSlot, IndexStoreSlot, IndexWatcherSlot,
    PriorityFilesSlot,
};
use crate::services::project_open;
use crate::services::repo_index::RepositoryIndex;
use crate::services::workspace_index::WorkspaceIndex;

pub const SYNC_PROGRESS_EVENT: &str = "embedding:sync-progress";

pub const MODEL_DOWNLOAD_PROGRESS_EVENT: &str = "embedding:model-download-progress";

/// Adapts `services::embedding_model`'s progress reports to a real Tauri
/// event — the one place `MODEL_DOWNLOAD_PROGRESS_EVENT` is emitted.
fn model_download_sink(app: &AppHandle) -> ModelDownloadSink {
    let app = app.clone();
    Arc::new(move |p: ModelDownloadProgress| {
        let _ = app.emit(MODEL_DOWNLOAD_PROGRESS_EVENT, p);
    })
}

/// Adapts `services::embedding_sync`'s `ProgressSink` to a real Tauri event.
/// This is the only place `SYNC_PROGRESS_EVENT` is emitted — the sync
/// pipeline itself has no `AppHandle` and no idea a UI is listening.
fn progress_sink(app: &AppHandle) -> ProgressSink {
    let app = app.clone();
    Arc::new(move |p: SyncProgress| {
        let _ = app.emit(SYNC_PROGRESS_EVENT, p);
    })
}

/// Unwraps the managed slots into the aggregate `services::embedding_sync`
/// works with. Both index use-cases need the identical set, so building it
/// in one place keeps the two commands from drifting apart.
#[allow(clippy::too_many_arguments)]
fn session_from_state(
    repo_index: &State<'_, Arc<RepositoryIndex>>,
    chunk_index: &State<'_, Arc<ChunkIndex>>,
    embedding_index: &State<'_, Arc<EmbeddingIndexSlot>>,
    index_store: &State<'_, Arc<IndexStoreSlot>>,
    embedding_provider: &State<'_, Arc<EmbeddingProviderSlot>>,
    sync_guard: &State<'_, Arc<EmbeddingSyncGuard>>,
    index_watcher: &State<'_, Arc<IndexWatcherSlot>>,
    workspace_index: &State<'_, Arc<WorkspaceIndex>>,
    priority_files: &State<'_, Arc<PriorityFilesSlot>>,
    background_backlog: &State<'_, Arc<BackgroundBacklogSlot>>,
    full_sync_active: &State<'_, Arc<FullSyncActiveSlot>>,
) -> EmbeddingSession {
    EmbeddingSession {
        repo_index: repo_index.inner().clone(),
        chunk_index: chunk_index.inner().clone(),
        embedding_index: embedding_index.inner().clone(),
        index_store: index_store.inner().clone(),
        embedding_provider: embedding_provider.inner().clone(),
        sync_guard: sync_guard.inner().clone(),
        index_watcher: index_watcher.inner().clone(),
        workspace_index: workspace_index.inner().clone(),
        priority_files: priority_files.inner().clone(),
        background_backlog: background_backlog.inner().clone(),
        full_sync_active: full_sync_active.inner().clone(),
    }
}

/// Returns the **resolved** embedding config (bundled preset merged with
/// the settings-layer override) — what the UI and runtime actually use.
#[tauri::command]
pub fn embedding_get_config() -> Result<ResolvedEmbeddingConfig, String> {
    embedding_config::resolve_embedding_config().map_err(|e| e.to_string())
}

/// Persists a settings-layer **override**. Pass explicit `Some` fields to
/// pin values; `None` means inherit from the bundled preset on the next
/// resolve.
#[tauri::command]
pub fn embedding_set_config(config: EmbeddingProviderConfig) -> Result<(), String> {
    embedding_config::save_embedding_settings(config).map_err(|e| e.to_string())
}

/// Write-only, mirrors `git_save_credentials`/`git_get_key_status`: the key
/// itself is never returned from a command, only whether one is set.
#[tauri::command]
pub fn embedding_set_remote_api_key(api_key: String) -> Result<(), String> {
    embedding_credentials_store::save_api_key(&api_key)
}

#[tauri::command]
pub fn embedding_has_remote_api_key() -> bool {
    embedding_credentials_store::has_api_key()
}

/// Drops the stored key entirely, leaving the rest of the remote config
/// (base URL, model, cert) in place — the Settings UI's counterpart to
/// `embedding_set_remote_api_key`, for switching a remote provider off
/// without wiping how to reach it.
///
/// No provider-cache invalidation needed here: `services::embedding_state::
/// ensure_provider` keys its cached instance on `(config, api_key)`, and the
/// next call reads `None` from the store, which no longer matches the cached
/// `Some(..)` — so the provider is rebuilt on its own.
#[tauri::command]
pub fn embedding_delete_remote_api_key() -> Result<(), String> {
    embedding_credentials_store::delete_api_key()
}

#[tauri::command]
pub fn embedding_model_status() -> ModelStatus {
    embedding_model::model_status()
}

#[tauri::command]
pub async fn embedding_download_model(
    app: AppHandle,
    state: State<'_, Arc<DownloadState>>,
) -> Result<(), String> {
    let state = state.inner().clone();
    let progress = model_download_sink(&app);
    tauri::async_runtime::spawn_blocking(move || {
        embedding_model::download_model(&progress, &state).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// `fastembed`'s blocking download has no interrupt hook — this can't stop
/// the in-flight network I/O, only tell the UI (and any progress events
/// from the attempt still running in the background) to stop trusting it.
/// See `DownloadState`'s doc comment for the full reasoning.
#[tauri::command]
pub fn embedding_cancel_model_download(state: State<'_, Arc<DownloadState>>) {
    embedding_model::cancel_download(&state);
}

/// IPC entry point for `services::embedding_sync::sync` — see it for what a
/// sync actually does. This adds only the two things the service can't: the
/// `spawn_blocking` hop (a sync can run model inference, comparable in cost
/// to `check_standards`/`ai_execute_tool`, so it must stay off the async
/// runtime) and a `ProgressSink` that turns the service's progress reports
/// into `SYNC_PROGRESS_EVENT`.
#[tauri::command]
pub async fn embedding_sync(
    app: AppHandle,
    repo_index: State<'_, Arc<RepositoryIndex>>,
    chunk_index: State<'_, Arc<ChunkIndex>>,
    embedding_index: State<'_, Arc<EmbeddingIndexSlot>>,
    index_store: State<'_, Arc<IndexStoreSlot>>,
    embedding_provider: State<'_, Arc<EmbeddingProviderSlot>>,
    sync_guard: State<'_, Arc<EmbeddingSyncGuard>>,
    index_watcher: State<'_, Arc<IndexWatcherSlot>>,
    workspace_index: State<'_, Arc<WorkspaceIndex>>,
    priority_files: State<'_, Arc<PriorityFilesSlot>>,
    background_backlog: State<'_, Arc<BackgroundBacklogSlot>>,
    full_sync_active: State<'_, Arc<FullSyncActiveSlot>>,
) -> Result<SyncStats, String> {
    let session = session_from_state(
        &repo_index,
        &chunk_index,
        &embedding_index,
        &index_store,
        &embedding_provider,
        &sync_guard,
        &index_watcher,
        &workspace_index,
        &priority_files,
        &background_backlog,
        &full_sync_active,
    );
    let progress = progress_sink(&app);

    tauri::async_runtime::spawn_blocking(move || embedding_sync::sync(&session, &progress))
        .await
        .map_err(|e| e.to_string())?
}

/// IPC entry point for `services::embedding_sync::status`, the read-only
/// counterpart to `embedding_sync` — same `spawn_blocking` + progress-sink
/// wiring, no logic of its own. Takes the same managed state as
/// `embedding_sync` because the warm-up it performs can start the
/// incremental watcher, which needs it.
#[tauri::command]
pub async fn embedding_index_status(
    app: AppHandle,
    repo_index: State<'_, Arc<RepositoryIndex>>,
    chunk_index: State<'_, Arc<ChunkIndex>>,
    embedding_index: State<'_, Arc<EmbeddingIndexSlot>>,
    index_store: State<'_, Arc<IndexStoreSlot>>,
    embedding_provider: State<'_, Arc<EmbeddingProviderSlot>>,
    sync_guard: State<'_, Arc<EmbeddingSyncGuard>>,
    index_watcher: State<'_, Arc<IndexWatcherSlot>>,
    workspace_index: State<'_, Arc<WorkspaceIndex>>,
    priority_files: State<'_, Arc<PriorityFilesSlot>>,
    background_backlog: State<'_, Arc<BackgroundBacklogSlot>>,
    full_sync_active: State<'_, Arc<FullSyncActiveSlot>>,
) -> Result<EmbeddingIndexStatus, String> {
    let session = session_from_state(
        &repo_index,
        &chunk_index,
        &embedding_index,
        &index_store,
        &embedding_provider,
        &sync_guard,
        &index_watcher,
        &workspace_index,
        &priority_files,
        &background_backlog,
        &full_sync_active,
    );
    let progress = progress_sink(&app);

    tauri::async_runtime::spawn_blocking(move || embedding_sync::status(&session, &progress))
        .await
        .map_err(|e| e.to_string())?
}

/// Stops the incremental file-watcher, if one is running. Called from the
/// frontend when a project closes without a new one opening in the same
/// session — otherwise `ensure_incremental_watcher`'s own `index_root`
/// check naturally swaps it for whichever project opens next. Dropping the
/// held `IndexWatcher` stops its underlying `notify` watch (RAII).
#[tauri::command]
pub fn embedding_index_teardown(
    index_watcher: State<'_, Arc<IndexWatcherSlot>>,
) -> Result<(), String> {
    *index_watcher
        .lock()
        .map_err(|_| "index watcher lock poisoned".to_string())? = None;
    Ok(())
}

/// Records which files are currently open in the editor, for
/// `embedding_sync`'s first-sync branch to prioritize (see
/// `PriorityFilesSlot`). `relative_paths` are exactly `EditorTab.path`
/// values — already relative to `project.docs_root` — so the frontend
/// never needs to know about `AiAccessMode`/`index_root`; this joins each
/// one against `docs_root` and relativizes it against whatever `index_root`
/// currently resolves to. A no-op (not an error) if no project is open, or
/// if a given path can't be resolved (e.g. a tab open on a file that was
/// just deleted) — this is a best-effort hint, never load-bearing for
/// correctness.
#[tauri::command]
pub fn embedding_set_priority_files(
    priority_files: State<'_, Arc<PriorityFilesSlot>>,
    relative_paths: Vec<String>,
) -> Result<(), String> {
    let Some(project) = project_open::get_project().map_err(|e| e.to_string())? else {
        return Ok(());
    };
    let (index_root, _) = resolve_index_paths(&project)?;
    let docs_root = PathBuf::from(&project.docs_root);

    let ids: HashSet<FileId> = relative_paths
        .iter()
        .filter_map(|rel| {
            let absolute = paths::join_relative(&docs_root, rel).ok()?;
            paths::relative_to_lenient(&index_root, &absolute)
                .ok()
                .map(FileId)
        })
        .collect();

    *priority_files
        .lock()
        .map_err(|_| "priority files lock poisoned".to_string())? = ids;
    Ok(())
}
