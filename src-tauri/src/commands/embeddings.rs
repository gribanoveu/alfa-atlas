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
/// `ChunkIndex` and `EmbeddingIndex`'s persistence for that project. The
/// `bool` mirrors `index_store_ensure::IndexAttachment::stale` at the time
/// of the last attach — cached here so a later `embedding_index_status`
/// call in the same session doesn't need to re-derive it, and so
/// `embedding_sync` can flip it to `false` in place once it actually
/// repairs a stale store.
pub type IndexStoreSlot = Mutex<Option<(PathBuf, Arc<IndexStore>, bool)>>;

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

/// Resolves both paths a project's index needs:
/// - `index_root` — same access-mode boundary `ai_execute_tool` already
///   respects (`services::ai_tools::current_scope`): `DocsOnly` (the
///   default) walks just the docs subtree, not the whole backend repo.
///   This is what `RepositoryIndex`/`ChunkBuilder`/`chunk_text::resolve_text`
///   resolve relative `FileId`s against, and what keys the
///   `ChunkIndex`/`EmbeddingIndexSlot` attach state.
/// - `storage_dir` — where that mode's persisted index lives on disk:
///   always under `{project.root}/.atlas/index/{mode}`, **never** under
///   `docs_root` — `.atlas` is the one place this app keeps per-project
///   state (`infra::project_store`'s `project.json` already lives there),
///   and nesting a second one under the docs subtree would split that
///   convention for no reason. The `{mode}` subfolder keeps `DocsOnly` and
///   `FullRepo` persisted separately (same reason `index_root` differs
///   between them — see `index_store_ensure` module docs).
fn resolve_index_paths(project: &OpenedProject) -> Result<(PathBuf, PathBuf), String> {
    let config = project_store::load(&project.root)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| ProjectConfig::new(project.docs_root.clone()));
    let (index_root, mode_dir) = match config.ai_access_mode {
        AiAccessMode::DocsOnly => (PathBuf::from(&project.docs_root), "docs-only"),
        AiAccessMode::FullRepo => (PathBuf::from(&project.root), "full-repo"),
    };
    let storage_dir = PathBuf::from(&project.root)
        .join(".atlas")
        .join("index")
        .join(mode_dir);
    Ok((index_root, storage_dir))
}

/// Attaches `index_root`'s persisted `IndexStore` to `chunk_index`. Only on
/// a genuine cold start or project/access-mode switch (the resident
/// `ChunkIndex` wasn't already tracking `index_root`) does this call
/// `index_store_ensure::open_for` at all — every later call in the same
/// session reuses what's already attached (and its cached `stale` flag)
/// instead of re-deriving it. Read-only — never walks the repo, never
/// touches the embedding provider, never mutates the store; `embedding_sync`
/// and `embedding_index_status` both build on this before doing their own,
/// different, work.
///
/// If the store is stale (see `index_store_ensure`), `chunk_index` is
/// deliberately left empty rather than loaded from metadata that might
/// describe an incompatible chunking algorithm or a different
/// `index_root` — the caller decides what to do with a stale attach
/// (`embedding_sync` repairs it; `embedding_index_status` just reports it).
fn attach_index_store(
    chunk_index: &ChunkIndex,
    index_store: &IndexStoreSlot,
    storage_dir: &Path,
    index_root: &Path,
) -> Result<(Arc<IndexStore>, bool), String> {
    let mut store_slot = index_store
        .lock()
        .map_err(|_| "index store lock poisoned".to_string())?;
    let is_new_attach = !matches!(store_slot.as_ref(), Some((root, _, _)) if root == index_root);
    if is_new_attach {
        let attachment = index_store_ensure::open_for(storage_dir, index_root)?;
        if !attachment.stale {
            chunk_index.load_metadata(attachment.store.load_all_chunks().map_err(|e| e.to_string())?);
        }
        *store_slot = Some((index_root.to_path_buf(), Arc::new(attachment.store), attachment.stale));
    }
    let (_, store, stale) = store_slot.as_ref().expect("just set above if it was missing");
    Ok((store.clone(), *stale))
}

/// Attaches `index_root` + `dimensions`'s `EmbeddingIndex` to the managed
/// slot — reusing what's already resident when both match, otherwise
/// reloading from `store` (`vectors.usearch` + the SQLite `chunk_hash`
/// mirror) when compatible, or starting blank when there's nothing
/// (compatible) to reload. Never embeds anything itself.
///
/// `allow_repair` gates what happens on a *dimension* mismatch (different
/// from `IndexStore`-level staleness — this is "the persisted vectors were
/// written for a different embedding provider/model"): `true` (only from
/// `embedding_sync`, already a real mutating sync) drops the mismatched
/// `vectors.usearch`/`embeddings` rows so a fresh embed can start clean;
/// `false` (from the read-only `embedding_index_status`) just returns a
/// blank in-memory index without touching disk, leaving whatever's
/// persisted for that other dimension alone.
fn attach_embedding_index(
    embedding_index: &EmbeddingIndexSlot,
    store: &IndexStore,
    index_root: &Path,
    dimensions: usize,
    allow_repair: bool,
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
        } else if allow_repair {
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
        } else {
            // Read-only path: report as empty for this dimension without
            // touching whatever's actually persisted on disk.
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
        let (index_root, storage_dir) = resolve_index_paths(&project)?;
        let (store, stale) = attach_index_store(&chunk_index, &index_store, &storage_dir, &index_root)?;
        if stale {
            // A real, already-mutating sync is the only place staleness
            // actually gets repaired (see `index_store_ensure` module docs)
            // — `chunk_index` is still empty from the attach above, so the
            // diff loop below naturally treats every current file as new.
            index_store_ensure::repair_stale(&store, &index_root)?;
            let mut store_slot = index_store
                .lock()
                .map_err(|_| "index store lock poisoned".to_string())?;
            if let Some(entry) = store_slot.as_mut() {
                entry.2 = false;
            }
        }

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

        attach_embedding_index(&embedding_index, &store, &index_root, dimensions, true)?;
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
/// e.g. right when a project opens, so the app knows the state without
/// waiting for the user to open a specific panel. Attaches (and, on a cold
/// start, reloads from disk) the same `ChunkIndex`/`EmbeddingIndex` state
/// `embedding_sync` would use, but never walks the repo, never repairs a
/// stale store, and never constructs a real `EmbeddingProvider` — dimension
/// lookup goes through `embedding_providers::expected_dimensions` (a plain
/// config read) instead of `provider_for`, specifically so this stays cheap
/// even for the Local provider (which would otherwise load the ~570MB ONNX
/// model just to read a constant). If no project is open, reports
/// `synced: false` rather than erroring — there is nothing to be out of
/// sync with.
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
            return Ok(EmbeddingIndexStatus {
                synced: false,
                embedded_count: 0,
                stale: false,
            });
        };
        let (index_root, storage_dir) = resolve_index_paths(&project)?;
        let (store, stale) = attach_index_store(&chunk_index, &index_store, &storage_dir, &index_root)?;
        if stale {
            // Nothing trustworthy to attach an EmbeddingIndex to — report
            // staleness and stop, rather than repairing (that only happens
            // inside a real `embedding_sync`).
            return Ok(EmbeddingIndexStatus {
                synced: false,
                embedded_count: 0,
                stale: true,
            });
        }

        let config = embedding_config::load_embedding_config().map_err(|e| e.to_string())?;
        let dimensions = embedding_providers::expected_dimensions(&config);

        attach_embedding_index(&embedding_index, &store, &index_root, dimensions, false)?;
        let slot = embedding_index
            .lock()
            .map_err(|_| "embedding index lock poisoned".to_string())?;
        let (_, _, index) = slot.as_ref().expect("attach_embedding_index just set this");
        let embedded_count = index.len();
        Ok(EmbeddingIndexStatus {
            synced: embedded_count > 0,
            embedded_count,
            stale: false,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
