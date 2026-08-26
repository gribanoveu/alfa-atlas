use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::domain::chunk_index::ChunkBuildOptions;
use crate::domain::embeddings::{
    EmbeddingIndexStatus, EmbeddingProviderConfig, ModelStatus, ResolvedEmbeddingConfig,
    SyncPhase, SyncProgress, SyncStats, SyncTrigger,
};
use crate::domain::paths;
use crate::domain::repo_index::FileId;
use crate::infra::{embedding_credentials_store, embedding_providers};
use crate::services::chunk_builder::{ChunkBuilder, ChunkIndex};
use crate::services::embedding_config;
use crate::services::embedding_index::EmbeddingBuilder;
use crate::services::embedding_model::{self, DownloadState};
use crate::services::embedding_sync::{
    direct_dependencies, ensure_incremental_watcher, load_persisted_symbols,
    merge_background_backlog, run_background_backlog_sync, split_sync_tiers, ProgressSink,
};
use crate::services::embedding_state::{
    attach_embedding_index, attach_index_store, ensure_provider, is_current_index_root,
    lock_sync_guard, resolve_index_paths, BackgroundBacklogSlot, EmbeddingIndexSlot,
    EmbeddingProviderSlot, EmbeddingSyncGuard, FullSyncActiveGuard, FullSyncActiveSlot,
    IndexStoreSlot, IndexWatcherSlot, PriorityFilesSlot,
};
use crate::services::index_store_ensure;
use crate::services::project_open;
use crate::services::repo_index::RepositoryIndex;
use crate::services::workspace_index::WorkspaceIndex;

pub const SYNC_PROGRESS_EVENT: &str = "embedding:sync-progress";

/// Adapts `services::embedding_sync`'s `ProgressSink` to a real Tauri event.
/// This is the only place `SYNC_PROGRESS_EVENT` is emitted — the sync
/// pipeline itself has no `AppHandle` and no idea a UI is listening.
fn progress_sink(app: &AppHandle) -> ProgressSink {
    let app = app.clone();
    Arc::new(move |p: SyncProgress| {
        let _ = app.emit(SYNC_PROGRESS_EVENT, p);
    })
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
    embedding_provider: State<'_, Arc<EmbeddingProviderSlot>>,
    sync_guard: State<'_, Arc<EmbeddingSyncGuard>>,
    index_watcher: State<'_, Arc<IndexWatcherSlot>>,
    workspace_index: State<'_, Arc<WorkspaceIndex>>,
    priority_files: State<'_, Arc<PriorityFilesSlot>>,
    background_backlog: State<'_, Arc<BackgroundBacklogSlot>>,
    full_sync_active: State<'_, Arc<FullSyncActiveSlot>>,
) -> Result<SyncStats, String> {
    let repo_index = repo_index.inner().clone();
    let chunk_index = chunk_index.inner().clone();
    let embedding_index = embedding_index.inner().clone();
    let index_store = index_store.inner().clone();
    let embedding_provider = embedding_provider.inner().clone();
    let sync_guard = sync_guard.inner().clone();
    let index_watcher = index_watcher.inner().clone();
    let workspace_index = workspace_index.inner().clone();
    let priority_files = priority_files.inner().clone();
    let background_backlog = background_backlog.inner().clone();
    let full_sync_active = full_sync_active.inner().clone();
    let progress = progress_sink(&app);

    tauri::async_runtime::spawn_blocking(move || -> Result<SyncStats, String> {
        // Acquired first, before any other slot, and held for the entire
        // pipeline — see `EmbeddingSyncGuard`'s doc comment for why a full
        // sync and an incremental tick must never interleave.
        let _guard = lock_sync_guard(&sync_guard);

        let project = project_open::get_project()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no project is open".to_string())?;
        let (index_root, storage_dir) = resolve_index_paths(&project)?;
        // Soft-abort (Ok empty stats, not Err): the caller may already have
        // switched projects, and surfacing an error would paint the new
        // project's UI. Same policy as `run_background_backlog_sync`.
        if !is_current_index_root(&index_root) {
            return Ok(SyncStats::default());
        }
        // Rejects a concurrent branch checkout for the rest of this walk —
        // see `FullSyncActiveGuard`.
        let _full_sync_active = FullSyncActiveGuard::new(&full_sync_active);
        let (store, stale) = attach_index_store(&chunk_index, &index_store, &storage_dir, &index_root)?;

        // Started regardless of `stale` — harmless either way, since
        // `run_incremental_sync` no-ops until `RepositoryIndex` has a
        // baseline (established a few lines below by `repo_index.build`).
        ensure_incremental_watcher(
            &index_watcher,
            &progress,
            &index_root,
            &store,
            &repo_index,
            &chunk_index,
            &embedding_index,
            &embedding_provider,
            &sync_guard,
        )?;

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

        let persisted_symbols = load_persisted_symbols(&store)?;
        repo_index
            .build_reusing_symbols(&index_root, &persisted_symbols)
            .map_err(|e| e.to_string())?;

        if !is_current_index_root(&index_root) {
            return Ok(SyncStats::default());
        }

        // A fresh project (nothing chunked yet, in this store or ever) is
        // the only case that additionally prioritizes open editor files —
        // documentation itself is always prioritized below, on every sync.
        let is_first_sync = chunk_index.chunk_ids().is_empty();

        let current_ids = repo_index.file_ids();
        let current_set: HashSet<_> = current_ids.iter().cloned().collect();

        // Open editor files (plus their direct includes/xrefs, resolved via
        // `WorkspaceIndex`) get chunked+embedded first so a fresh project's
        // first sync returns quickly with a useful partial index. Empty on
        // anything but a first sync, and also empty whenever no priority
        // file survives the `current_set` intersection (nothing open, or a
        // stale `PriorityFilesSlot` snapshot — see that type's doc comment).
        let priority_ids: HashSet<FileId> = if is_first_sync {
            let opened = priority_files
                .lock()
                .map_err(|_| "priority files lock poisoned".to_string())?
                .clone();
            let mut set = opened.clone();
            for file_id in &opened {
                set.extend(direct_dependencies(&workspace_index, file_id));
                // Java's import graph lives directly in `FileId` space (no
                // `WorkspaceIndex`/`DocumentId` translation needed — `.java`
                // is never a `WorkspaceIndex` document) — see
                // `RepositoryIndex::java_dependencies`.
                set.extend(repo_index.java_dependencies(file_id));
            }
            set.retain(|id| current_set.contains(id));
            set
        } else {
            HashSet::new()
        };

        // Documentation changes always sync ahead of the rest of the repo
        // (`project.docs_root`) — every call, not just the first — with any
        // remaining non-doc backlog either folded in here too (small
        // change sets) or deferred to the background (large ones). See
        // `split_sync_tiers`.
        let (tier1_ids, tier2_ids) = split_sync_tiers(
            &current_ids,
            &chunk_index,
            &repo_index,
            &project.docs_root,
            &priority_ids,
        );

        let chunk_builder = ChunkBuilder::new();
        let options = ChunkBuildOptions::default();

        // Only files whose content hash moved since `ChunkIndex` last saw
        // them get re-chunked — for the rest, `build_file` (and the file
        // read it requires) is skipped entirely, and `EmbeddingIndex::sync`
        // below will correctly see their chunks' hashes as unchanged
        // without this sync ever touching their text. Scoped to `tier1_ids`
        // — `tier2_ids` is handled by the background backlog task, not
        // here.
        let mut changed_files = Vec::new();
        for file_id in &tier1_ids {
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
        for file_id in &tier1_ids {
            if !is_current_index_root(&index_root) {
                return Ok(SyncStats::default());
            }
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
            store
                .replace_symbols_for_file(file_id, &indexed.symbols)
                .map_err(|e| e.to_string())?;
            store
                .replace_imports_for_file(file_id, &indexed.imports)
                .map_err(|e| e.to_string())?;
            chunked_so_far += 1;
            progress(SyncProgress {
                phase: SyncPhase::Chunking,
                current: chunked_so_far,
                total: total_changed,
                trigger: SyncTrigger::Full,
            });
        }

        if !is_current_index_root(&index_root) {
            return Ok(SyncStats::default());
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

        let config = embedding_config::resolve_embedding_config().map_err(|e| e.to_string())?;
        let api_key = embedding_credentials_store::get_api_key();
        let provider = ensure_provider(&embedding_provider, &config, api_key)?;
        let dimensions = provider.dimensions();
        eprintln!(
            "[embedding] syncing via {:?} provider ({:?}, {dimensions} dims)",
            config.kind, config.remote_model
        );
        let builder = EmbeddingBuilder::new(provider);

        if !is_current_index_root(&index_root) {
            return Ok(SyncStats::default());
        }

        attach_embedding_index(&embedding_index, &store, &index_root, dimensions, true)?;
        let stats = {
            let mut slot = embedding_index
                .lock()
                .map_err(|_| "embedding index lock poisoned".to_string())?;
            let (_, _, index) = slot.as_mut().expect("attach_embedding_index just set this");
            let on_progress = |current: usize, total: usize| {
                progress(SyncProgress {
                    phase: SyncPhase::Embedding,
                    current,
                    total,
                    trigger: SyncTrigger::Full,
                });
            };
            index
                .sync(&chunk_index, &builder, &index_root, Some(&store), Some(&on_progress))
                .map_err(|e| e.to_string())?
        };

        if !is_current_index_root(&index_root) {
            // Chunk/embed work for the abandoned project is already on its
            // own disk; skip spawning a backlog that would keep mutating
            // shared in-memory slots after the switch.
            return Ok(stats);
        }

        // Any large non-doc backlog (a fresh project's first sync, or a
        // routine sync catching up after a big upstream change), merged
        // into whatever this project's background queue already has and
        // dispatched to its own task only if nothing is draining it yet —
        // see `merge_background_backlog`/`run_background_backlog_sync`'s
        // doc comments for why a fixed-`Vec` dispatch isn't safe once this
        // can fire on every sync, not just the first.
        if !tier2_ids.is_empty() {
            let should_spawn = merge_background_backlog(&background_backlog, &index_root, tier2_ids)?;
            if should_spawn {
                let repo_index = repo_index.clone();
                let chunk_index = chunk_index.clone();
                let embedding_index = embedding_index.clone();
                let embedding_provider = embedding_provider.clone();
                let sync_guard = sync_guard.clone();
                let store = store.clone();
                let index_root = index_root.clone();
                let progress = progress.clone();
                let background_backlog = background_backlog.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    run_background_backlog_sync(
                        repo_index,
                        chunk_index,
                        embedding_index,
                        embedding_provider,
                        sync_guard,
                        store,
                        index_root,
                        progress,
                        background_backlog,
                    );
                });
            }
        }

        Ok(stats)
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
    app: AppHandle,
    repo_index: State<'_, Arc<RepositoryIndex>>,
    chunk_index: State<'_, Arc<ChunkIndex>>,
    embedding_index: State<'_, Arc<EmbeddingIndexSlot>>,
    index_store: State<'_, Arc<IndexStoreSlot>>,
    embedding_provider: State<'_, Arc<EmbeddingProviderSlot>>,
    sync_guard: State<'_, Arc<EmbeddingSyncGuard>>,
    index_watcher: State<'_, Arc<IndexWatcherSlot>>,
    background_backlog: State<'_, Arc<BackgroundBacklogSlot>>,
) -> Result<EmbeddingIndexStatus, String> {
    let repo_index = repo_index.inner().clone();
    let chunk_index = chunk_index.inner().clone();
    let embedding_index = embedding_index.inner().clone();
    let index_store = index_store.inner().clone();
    let embedding_provider = embedding_provider.inner().clone();
    let sync_guard = sync_guard.inner().clone();
    let index_watcher = index_watcher.inner().clone();
    let background_backlog = background_backlog.inner().clone();
    let progress = progress_sink(&app);

    tauri::async_runtime::spawn_blocking(move || -> Result<EmbeddingIndexStatus, String> {
        // Same guard `embedding_sync` acquires first — attach swaps the
        // shared `ChunkIndex`/`IndexStoreSlot`/`EmbeddingIndexSlot`, so a
        // status warm-up on project open must never race an in-flight full
        // sync (or incremental tick) that still holds those for the previous
        // project. Waits the sync out rather than interleaving.
        let _guard = lock_sync_guard(&sync_guard);

        let Some(project) = project_open::get_project().map_err(|e| e.to_string())? else {
            return Ok(EmbeddingIndexStatus {
                synced: false,
                embedded_count: 0,
                stale: false,
                background_pending: 0,
            });
        };
        let (index_root, storage_dir) = resolve_index_paths(&project)?;
        let (store, stale) = attach_index_store(&chunk_index, &index_store, &storage_dir, &index_root)?;

        // Eager warm-up: this read-only status check is what
        // `useEmbeddingIndexWarmup` calls right when a project opens, so
        // starting the watcher here (rather than only inside
        // `embedding_sync`) is what makes incremental watching begin
        // immediately instead of waiting for the user's first manual sync.
        // Started regardless of `stale` — harmless, `run_incremental_sync`
        // no-ops until `RepositoryIndex` has a baseline.
        ensure_incremental_watcher(
            &index_watcher,
            &progress,
            &index_root,
            &store,
            &repo_index,
            &chunk_index,
            &embedding_index,
            &embedding_provider,
            &sync_guard,
        )?;

        if stale {
            // Nothing trustworthy to attach an EmbeddingIndex to — report
            // staleness and stop, rather than repairing (that only happens
            // inside a real `embedding_sync`).
            return Ok(EmbeddingIndexStatus {
                synced: false,
                embedded_count: 0,
                stale: true,
                background_pending: 0,
            });
        }

        let config = embedding_config::resolve_embedding_config().map_err(|e| e.to_string())?;
        let dimensions = embedding_providers::expected_dimensions(&config);

        attach_embedding_index(&embedding_index, &store, &index_root, dimensions, false)?;
        let slot = embedding_index
            .lock()
            .map_err(|_| "embedding index lock poisoned".to_string())?;
        let (_, _, index) = slot.as_ref().expect("attach_embedding_index just set this");
        let embedded_count = index.len();
        // Whatever `run_background_backlog_sync` still has left to process
        // for *this* project — `0` if nothing's ever been queued, or if the
        // slot currently belongs to a different `index_root` (a stale entry
        // some other project's sync will reclaim/replace on its own, not
        // this one's to report). See `EmbeddingIndexStatus::
        // background_pending`'s doc comment.
        let background_pending = background_backlog
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .filter(|b| b.index_root == index_root)
                    .map(|b| b.pending.len())
            })
            .unwrap_or(0);
        Ok(EmbeddingIndexStatus {
            synced: embedded_count > 0,
            embedded_count,
            stale: false,
            background_pending,
        })
    })
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
