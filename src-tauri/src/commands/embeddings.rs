use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, State};

use crate::domain::ai_access::AiAccessMode;
use crate::domain::chunk_index::ChunkBuildOptions;
use crate::domain::embeddings::{EmbeddingIndexStatus, EmbeddingProviderConfig, ModelStatus, SyncStats};
use crate::domain::project_config::{OpenedProject, ProjectConfig};
use crate::infra::index_store::IndexStore;
use crate::infra::{embedding_credentials_store, embedding_providers, project_store};
use crate::services::chunk_builder::{ChunkBuilder, ChunkIndex};
use crate::services::embedding_config;
use crate::services::embedding_index::{EmbeddingBuilder, EmbeddingIndex};
use crate::services::embedding_model::{self, DownloadState};
use crate::services::index_store_ensure;
use crate::services::project_open;
use crate::services::repo_index::RepositoryIndex;

const META_EMBEDDING_DIMENSIONS: &str = "embedding_dimensions";

pub const SYNC_PROGRESS_EVENT: &str = "embedding:sync-progress";

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum SyncPhase {
    /// Re-chunking files whose content hash changed since the last sync —
    /// fast (no network/inference), but still worth reporting since a
    /// large `FullRepo` change set can take a few seconds on its own.
    Chunking,
    /// Calling the embedding provider for pending chunks, in batches of
    /// `EMBED_PROGRESS_BATCH` — the slow phase (network or ONNX inference).
    Embedding,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncProgressPayload {
    phase: SyncPhase,
    current: usize,
    total: usize,
}

fn emit_sync_progress(app: &AppHandle, phase: SyncPhase, current: usize, total: usize) {
    let _ = app.emit(SYNC_PROGRESS_EVENT, SyncProgressPayload { phase, current, total });
}

/// `EmbeddingIndex` can't be built until a provider (hence a dimension
/// count) is known, and switching provider can change that dimension — so
/// the managed state is a lazily-(re)built slot, not a bare `EmbeddingIndex`
/// constructed once at app startup like `RepositoryIndex`/`ChunkIndex` are.
/// Keyed by `(index_root, dimensions)` — either changing (a different
/// project/access-mode opened, or the provider's dimension count changed)
/// invalidates the resident index the same way.
pub type EmbeddingIndexSlot = Mutex<Option<(PathBuf, usize, EmbeddingIndex)>>;

/// One `IndexStore` (SQLite connection) per `index_root`, shared by
/// `ChunkIndex` and `EmbeddingIndex`'s persistence for that project.
pub type IndexStoreSlot = Mutex<Option<(PathBuf, Arc<IndexStore>)>>;

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

/// Same access-mode boundary `ai_execute_tool` already respects
/// (`services::ai_tools::current_scope`) — `DocsOnly` (the default) indexes
/// just the docs subtree, not the whole backend repo.
fn resolve_index_root(project: &OpenedProject) -> Result<PathBuf, String> {
    let config = project_store::load(&project.root)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| ProjectConfig::new(project.docs_root.clone()));
    Ok(match config.ai_access_mode {
        AiAccessMode::DocsOnly => PathBuf::from(&project.docs_root),
        AiAccessMode::FullRepo => PathBuf::from(&project.root),
    })
}

/// Attaches `index_root`'s persisted `IndexStore` to `chunk_index`. Only on
/// a genuine cold start or project/access-mode switch (the resident
/// `ChunkIndex` wasn't already tracking `index_root`) does this bulk-reload
/// its metadata from SQLite instead of reusing what's already resident.
/// Read-only otherwise — never walks the repo, never touches the embedding
/// provider; `embedding_sync` and `embedding_index_status` both build on
/// this before doing their own, different, work.
fn attach_index_store(
    chunk_index: &ChunkIndex,
    index_store: &IndexStoreSlot,
    index_root: &Path,
) -> Result<Arc<IndexStore>, String> {
    let mut store_slot = index_store
        .lock()
        .map_err(|_| "index store lock poisoned".to_string())?;
    let is_new_attach = !matches!(store_slot.as_ref(), Some((root, _)) if root == index_root);
    if is_new_attach {
        let store = Arc::new(index_store_ensure::open_for(index_root)?);
        chunk_index.load_metadata(store.load_all_chunks().map_err(|e| e.to_string())?);
        *store_slot = Some((index_root.to_path_buf(), store));
    }
    Ok(store_slot.as_ref().expect("just set above if it was missing").1.clone())
}

/// Attaches `index_root` + `dimensions`'s `EmbeddingIndex` to the managed
/// slot — reusing what's already resident when both match, otherwise
/// reloading from `store` (`vectors.usearch` + the SQLite `chunk_hash`
/// mirror) when compatible, or starting blank when there's nothing
/// (compatible) to reload. Never embeds anything itself.
fn attach_embedding_index(
    embedding_index: &EmbeddingIndexSlot,
    store: &IndexStore,
    index_root: &Path,
    dimensions: usize,
) -> Result<(), String> {
    let mut slot = embedding_index
        .lock()
        .map_err(|_| "embedding index lock poisoned".to_string())?;
    let needs_reload =
        !matches!(slot.as_ref(), Some((root, d, _)) if root == index_root && *d == dimensions);
    if needs_reload {
        let persisted_dimensions: Option<usize> = store
            .read_meta(META_EMBEDDING_DIMENSIONS)
            .map_err(|e| e.to_string())?
            .and_then(|s| s.parse().ok());

        let fresh = if persisted_dimensions == Some(dimensions) {
            let persisted_hashes = store.load_all_embedding_hashes().map_err(|e| e.to_string())?;
            EmbeddingIndex::load(dimensions, &store.vectors_path(), persisted_hashes)
                .map_err(|e| e.to_string())?
        } else {
            // No persisted vectors for this dimension (first sync ever, or
            // the provider's dimension changed since last time) — whatever
            // is on disk for a *different* dimension can't be reused, so
            // drop it rather than risk loading it anyway.
            store.clear_embeddings().map_err(|e| e.to_string())?;
            let vectors_path = store.vectors_path();
            if vectors_path.exists() {
                std::fs::remove_file(&vectors_path).map_err(|e| e.to_string())?;
            }
            store
                .write_meta(META_EMBEDDING_DIMENSIONS, &dimensions.to_string())
                .map_err(|e| e.to_string())?;
            EmbeddingIndex::new(dimensions).map_err(|e| e.to_string())?
        };
        *slot = Some((index_root.to_path_buf(), dimensions, fresh));
    }
    Ok(())
}

/// Walks `RepositoryIndex` for the currently open project (full rescan —
/// cheap relative to embedding inference: hashing + tree-sitter parsing,
/// no network/ONNX), then re-chunks only files whose content hash changed
/// since `ChunkIndex` last saw them, then reconciles `EmbeddingIndex`
/// against the result (`EmbeddingIndex::sync` — new chunk embedded,
/// changed-hash chunk re-embedded, deleted chunk's vector removed). Both
/// `ChunkIndex` and `EmbeddingIndex` are mirrored to a per-project SQLite
/// store (`infra::index_store`) + `vectors.usearch` file, so a later
/// restart reloads this state instead of re-walking/re-embedding from
/// scratch. `spawn_blocking`: this can run model inference, comparable in
/// cost to `check_standards`/`ai_execute_tool`.
#[tauri::command]
pub async fn embedding_sync(
    app: AppHandle,
    repo_index: State<'_, Arc<RepositoryIndex>>,
    chunk_index: State<'_, Arc<ChunkIndex>>,
    embedding_index: State<'_, Arc<EmbeddingIndexSlot>>,
    index_store: State<'_, Arc<IndexStoreSlot>>,
) -> Result<SyncStats, String> {
    let repo_index = repo_index.inner().clone();
    let chunk_index = chunk_index.inner().clone();
    let embedding_index = embedding_index.inner().clone();
    let index_store = index_store.inner().clone();

    tauri::async_runtime::spawn_blocking(move || -> Result<SyncStats, String> {
        let project = project_open::get_project()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no project is open".to_string())?;
        let index_root = resolve_index_root(&project)?;
        let store = attach_index_store(&chunk_index, &index_store, &index_root)?;

        repo_index.build(&index_root).map_err(|e| e.to_string())?;

        let current_ids = repo_index.file_ids();
        let current_set: HashSet<_> = current_ids.iter().cloned().collect();
        let chunk_builder = ChunkBuilder::new();
        let options = ChunkBuildOptions::default();

        // Only files whose content hash moved since `ChunkIndex` last saw
        // them get re-chunked — for the rest, `build_file` (and the file
        // read it requires) is skipped entirely, and `EmbeddingIndex::sync`
        // below will correctly see their chunks' hashes as unchanged
        // without this sync ever touching their text.
        let mut changed_files = Vec::new();
        for file_id in &current_ids {
            let Some(indexed) = repo_index.get(file_id) else {
                continue;
            };
            let unchanged = chunk_index
                .file_hash_for(file_id)
                .is_some_and(|hash| hash == indexed.metadata.hash);
            if !unchanged {
                changed_files.push(indexed.metadata.clone());
            }
        }
        if !changed_files.is_empty() {
            store.upsert_files(&changed_files).map_err(|e| e.to_string())?;
        }
        let total_changed = changed_files.len();
        let mut chunked_so_far = 0usize;
        for file_id in &current_ids {
            let Some(indexed) = repo_index.get(file_id) else {
                continue;
            };
            let unchanged = chunk_index
                .file_hash_for(file_id)
                .is_some_and(|hash| hash == indexed.metadata.hash);
            if unchanged {
                continue;
            }
            let chunks = chunk_builder
                .build_file(&repo_index, file_id, &options)
                .map_err(|e| e.to_string())?;
            let metadatas: Vec<_> = chunks.iter().map(|c| c.metadata.clone()).collect();
            chunk_index.replace_for_file(file_id, chunks);
            store
                .replace_chunks_for_file(file_id, &metadatas)
                .map_err(|e| e.to_string())?;
            chunked_so_far += 1;
            emit_sync_progress(&app, SyncPhase::Chunking, chunked_so_far, total_changed);
        }

        // Files present in `ChunkIndex` but gone from this scan — deleted
        // since the index was last built/loaded.
        let stale_file_ids: Vec<_> = chunk_index
            .file_ids()
            .into_iter()
            .filter(|id| !current_set.contains(id))
            .collect();
        for file_id in &stale_file_ids {
            chunk_index.replace_for_file(file_id, Vec::new());
        }
        if !stale_file_ids.is_empty() {
            // Cascades to that file's `chunks`/`embeddings` rows too.
            store.delete_files(&stale_file_ids).map_err(|e| e.to_string())?;
        }

        let config = embedding_config::load_embedding_config().map_err(|e| e.to_string())?;
        let api_key = embedding_credentials_store::get_api_key();
        let provider =
            embedding_providers::provider_for(&config, api_key).map_err(|e| e.to_string())?;
        let dimensions = provider.dimensions();
        let builder = EmbeddingBuilder::new(Arc::from(provider));

        attach_embedding_index(&embedding_index, &store, &index_root, dimensions)?;
        let mut slot = embedding_index
            .lock()
            .map_err(|_| "embedding index lock poisoned".to_string())?;
        let (_, _, index) = slot.as_mut().expect("attach_embedding_index just set this");
        let on_progress = |current: usize, total: usize| {
            emit_sync_progress(&app, SyncPhase::Embedding, current, total);
        };
        index
            .sync(&chunk_index, &builder, &index_root, Some(&store), Some(&on_progress))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Read-only counterpart to `embedding_sync`, for the UI to learn "is this
/// project's index already built" without triggering a rescan/re-embed —
/// e.g. on mount, so a remounted panel can show real persisted state
/// instead of resetting to "not yet synced". Attaches (and, on a cold
/// start, reloads from disk) the same `ChunkIndex`/`EmbeddingIndex` state
/// `embedding_sync` would use, but never walks the repo or calls the
/// embedding provider beyond resolving its configured dimension count. If
/// no project is open, reports `synced: false` rather than erroring — there
/// is nothing to be out of sync with.
#[tauri::command]
pub async fn embedding_index_status(
    chunk_index: State<'_, Arc<ChunkIndex>>,
    embedding_index: State<'_, Arc<EmbeddingIndexSlot>>,
    index_store: State<'_, Arc<IndexStoreSlot>>,
) -> Result<EmbeddingIndexStatus, String> {
    let chunk_index = chunk_index.inner().clone();
    let embedding_index = embedding_index.inner().clone();
    let index_store = index_store.inner().clone();

    tauri::async_runtime::spawn_blocking(move || -> Result<EmbeddingIndexStatus, String> {
        let Some(project) = project_open::get_project().map_err(|e| e.to_string())? else {
            return Ok(EmbeddingIndexStatus { synced: false, embedded_count: 0 });
        };
        let index_root = resolve_index_root(&project)?;
        let store = attach_index_store(&chunk_index, &index_store, &index_root)?;

        let config = embedding_config::load_embedding_config().map_err(|e| e.to_string())?;
        let api_key = embedding_credentials_store::get_api_key();
        let provider =
            embedding_providers::provider_for(&config, api_key).map_err(|e| e.to_string())?;
        let dimensions = provider.dimensions();

        attach_embedding_index(&embedding_index, &store, &index_root, dimensions)?;
        let slot = embedding_index
            .lock()
            .map_err(|_| "embedding index lock poisoned".to_string())?;
        let (_, _, index) = slot.as_ref().expect("attach_embedding_index just set this");
        let embedded_count = index.len();
        Ok(EmbeddingIndexStatus {
            synced: embedded_count > 0,
            embedded_count,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
