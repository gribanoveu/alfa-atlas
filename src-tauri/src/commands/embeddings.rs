use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, State};

use crate::domain::ai_access::AiAccessMode;
use crate::domain::chunk_index::ChunkBuildOptions;
use crate::domain::embeddings::{EmbeddingProviderConfig, ModelStatus, SyncStats};
use crate::domain::project_config::ProjectConfig;
use crate::infra::{embedding_credentials_store, embedding_providers, project_store};
use crate::services::chunk_builder::{ChunkBuilder, ChunkIndex};
use crate::services::embedding_config;
use crate::services::embedding_index::{EmbeddingBuilder, EmbeddingIndex};
use crate::services::embedding_model::{self, DownloadState};
use crate::services::project_open;
use crate::services::repo_index::RepositoryIndex;

/// `EmbeddingIndex` can't be built until a provider (hence a dimension
/// count) is known, and switching provider can change that dimension — so
/// the managed state is a lazily-(re)built slot, not a bare `EmbeddingIndex`
/// constructed once at app startup like `RepositoryIndex`/`ChunkIndex` are.
pub type EmbeddingIndexSlot = Mutex<Option<(usize, EmbeddingIndex)>>;

#[tauri::command]
pub fn embedding_get_config() -> Result<EmbeddingProviderConfig, String> {
    embedding_config::load_embedding_config().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn embedding_set_config(config: EmbeddingProviderConfig) -> Result<(), String> {
    embedding_config::save_embedding_config(config).map_err(|e| e.to_string())
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
    tauri::async_runtime::spawn_blocking(move || {
        embedding_model::download_model(&app, &state).map_err(|e| e.to_string())
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

/// Rebuilds `RepositoryIndex`/`ChunkIndex` for the currently open project,
/// then reconciles `EmbeddingIndex` against the result (`EmbeddingIndex::sync`
/// — new chunk embedded, changed-hash chunk re-embedded, deleted chunk's
/// vector removed). `spawn_blocking`: this walks the whole repo and can run
/// model inference, comparable in cost to `check_standards`/`ai_execute_tool`.
#[tauri::command]
pub async fn embedding_sync(
    repo_index: State<'_, Arc<RepositoryIndex>>,
    chunk_index: State<'_, Arc<ChunkIndex>>,
    embedding_index: State<'_, Arc<EmbeddingIndexSlot>>,
) -> Result<SyncStats, String> {
    let repo_index = repo_index.inner().clone();
    let chunk_index = chunk_index.inner().clone();
    let embedding_index = embedding_index.inner().clone();

    tauri::async_runtime::spawn_blocking(move || -> Result<SyncStats, String> {
        let project = project_open::get_project()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no project is open".to_string())?;
        // Same access-mode boundary `ai_execute_tool` already respects
        // (`services::ai_tools::current_scope`) — DocsOnly (the default)
        // indexes just the docs subtree, not the whole backend repo. Without
        // this, a real Java repo's every source file gets chunked and
        // embedded on every sync.
        let config = project_store::load(&project.root)
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| ProjectConfig::new(project.docs_root.clone()));
        let index_root = match config.ai_access_mode {
            AiAccessMode::DocsOnly => PathBuf::from(&project.docs_root),
            AiAccessMode::FullRepo => PathBuf::from(&project.root),
        };

        repo_index.build(&index_root).map_err(|e| e.to_string())?;
        let chunks = ChunkBuilder::new().build_all(&repo_index, &ChunkBuildOptions::default());
        chunk_index.clear();
        chunk_index.insert_all(chunks);

        let config = embedding_config::load_embedding_config().map_err(|e| e.to_string())?;
        let api_key = embedding_credentials_store::get_api_key();
        let provider =
            embedding_providers::provider_for(&config, api_key).map_err(|e| e.to_string())?;
        let dimensions = provider.dimensions();
        let builder = EmbeddingBuilder::new(Arc::from(provider));

        let mut slot = embedding_index
            .lock()
            .map_err(|_| "embedding index lock poisoned".to_string())?;
        let needs_rebuild = !matches!(slot.as_ref(), Some((d, _)) if *d == dimensions);
        if needs_rebuild {
            let fresh = EmbeddingIndex::new(dimensions).map_err(|e| e.to_string())?;
            *slot = Some((dimensions, fresh));
        }
        let (_, index) = slot.as_mut().expect("just set above if it was missing");
        index.sync(&chunk_index, &builder).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
