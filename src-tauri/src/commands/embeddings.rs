use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, State};

use crate::domain::chunk_index::ChunkBuildOptions;
use crate::domain::embeddings::{
    EmbeddingIndexStatus, EmbeddingProviderConfig, ModelStatus, ResolvedEmbeddingConfig,
    SyncStats,
};
use crate::domain::paths;
use crate::domain::repo_index::{detect_language, FileId, RepoIndexError};
use crate::domain::workspace_index::DocumentId;
use crate::infra::index_store::IndexStore;
use crate::infra::{embedding_credentials_store, embedding_providers, workspace_scanner};
use crate::services::chunk_builder::{ChunkBuilder, ChunkIndex};
use crate::services::embedding_config;
use crate::services::embedding_index::EmbeddingBuilder;
use crate::services::embedding_model::{self, DownloadState};
use crate::services::embedding_state::{
    attach_embedding_index, attach_index_store, ensure_provider, is_current_index_root,
    lock_sync_guard, resolve_index_paths, BackgroundBacklog, BackgroundBacklogSlot,
    EmbeddingIndexSlot, EmbeddingProviderSlot, EmbeddingSyncGuard, FullSyncActiveGuard,
    FullSyncActiveSlot, IndexStoreSlot, IndexWatcherSlot, PriorityFilesSlot,
    META_EMBEDDING_DIMENSIONS,
};
use crate::services::index_store_ensure;
use crate::services::index_watcher::{FileChangeKind, IndexWatcher};
use crate::services::project_open;
use crate::services::repo_index::{RepositoryIndex, ReusableFileData};
use crate::services::workspace_index::WorkspaceIndex;


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

/// Distinguishes a full, user-triggered `embedding_sync` from a
/// file-watcher-driven incremental tick — so the UI's "Синхронизировать"
/// progress display never mistakes a single background per-file update for
/// a full-repo resync in progress.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum SyncTrigger {
    Full,
    Incremental,
    /// The low-priority catch-up for the rest of a fresh project's files,
    /// running on its own task after the first sync's priority tier (open
    /// files + their direct includes/xrefs) already returned to the caller
    /// — see `run_background_backlog_sync`.
    Background,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncProgressPayload {
    phase: SyncPhase,
    current: usize,
    total: usize,
    trigger: SyncTrigger,
}

fn emit_sync_progress(
    app: &AppHandle,
    phase: SyncPhase,
    current: usize,
    total: usize,
    trigger: SyncTrigger,
) {
    let _ = app.emit(
        SYNC_PROGRESS_EVENT,
        SyncProgressPayload { phase, current, total, trigger },
    );
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


/// One open file's direct (one-hop, non-transitive) AsciiDoc dependencies —
/// its `include::`/`xref:` targets. Empty for a file `WorkspaceIndex`
/// doesn't know about (not built yet this session, or a non-AsciiDoc file
/// with no include/xref syntax) — graceful degradation, not an error: the
/// priority tier then just contains the open file itself. `WorkspaceIndex`
/// always walks `project.repoRoot` (see `commands::workspace_index::
/// build_index`), and `index_root` is now always `project.root` too, so a
/// `FileId` and a `WorkspaceIndex::DocumentId` are always the same
/// repo-relative string — no bridging needed (unlike before this change,
/// when `DocsOnly`'s `index_root` was `docs_root`, a strict subset of
/// `repoRoot`).
fn direct_dependencies(workspace_index: &WorkspaceIndex, file_id: &FileId) -> Vec<FileId> {
    let doc_id = DocumentId::new(file_id.0.clone());
    let mut out = Vec::new();
    for inc in workspace_index.find_includes(&doc_id) {
        out.push(FileId(inc.path));
    }
    for r in workspace_index.find_references(&doc_id) {
        if r.target_document.is_empty() {
            // Same-document `#anchor` xref — not a cross-file dependency.
            continue;
        }
        out.push(FileId(r.target_document));
    }
    out
}


/// Combines `store`'s persisted per-file metadata (content hash, size,
/// mtime, language) with its persisted symbols and imports into the shape
/// `RepositoryIndex::build_reusing_symbols` wants — what a fresh (e.g.
/// just-restarted) `embedding_sync` call feeds it so a file whose mtime/size
/// (cheapest check) or content hash (fallback) still match the last sync
/// skips a tree-sitter/pulldown-cmark re-parse entirely, not just
/// re-embedding — including its Java import graph, which used to be
/// silently dropped here (see `IndexStore::load_all_imports`'s doc comment).
fn load_persisted_symbols(store: &IndexStore) -> Result<HashMap<FileId, ReusableFileData>, String> {
    let files = store.load_all_files().map_err(|e| e.to_string())?;
    let mut symbols_by_file = store.load_all_symbols().map_err(|e| e.to_string())?;
    let mut imports_by_file = store.load_all_imports().map_err(|e| e.to_string())?;
    Ok(files
        .into_iter()
        .map(|(file_id, metadata)| {
            let symbols = symbols_by_file.remove(&file_id).unwrap_or_default();
            let imports = imports_by_file.remove(&file_id).unwrap_or_default();
            (file_id, ReusableFileData { metadata, symbols, imports })
        })
        .collect())
}

/// The file-watcher's `on_change` reaction — the incremental counterpart to
/// `embedding_sync`'s per-file diff loop, for exactly one changed path.
/// Called from `IndexWatcher`'s own `spawn_blocking` task (see that
/// module's docs), so this runs entirely synchronously and never blocks
/// Tauri's async runtime — matching requirement 4 (never block the UI or a
/// tool-call).
///
/// A genuinely new, untracked file (`repo_index.get` doesn't know about
/// `file_id` yet) is still indexed here, not deferred to the next full
/// sync — `workspace_scanner::is_new_file_indexable` answers the one thing
/// a full gitignore-aware walk would otherwise be needed for (is this path
/// actually hidden/gitignored, given `IndexWatcher`'s own `is_relevant`
/// filter is extension-only and doesn't know about `.gitignore`) far more
/// cheaply than re-walking the whole tree. A genuinely gitignored file
/// still never reaches `update_file` below. This also means nothing
/// happens here until `RepositoryIndex` has a baseline for this
/// `index_root` (at least one `embedding_sync` this session) — expected,
/// not a bug: `RepositoryIndex` has no persistence of its own.
///
/// `on_embedding_progress` is injected (mirrors `EmbeddingIndex::sync`'s own
/// callback-based design) rather than taking an `AppHandle` directly — this
/// keeps the actual sync logic testable with a no-op callback, with
/// `ensure_incremental_watcher`'s closure the only place that touches
/// `AppHandle` at all, translating the callback into a real
/// `embedding:sync-progress` event.
#[allow(clippy::too_many_arguments)]
fn run_incremental_sync(
    repo_index: &RepositoryIndex,
    chunk_index: &ChunkIndex,
    embedding_index: &EmbeddingIndexSlot,
    embedding_provider: &EmbeddingProviderSlot,
    sync_guard: &EmbeddingSyncGuard,
    index_root: &Path,
    store: &IndexStore,
    path: PathBuf,
    kind: FileChangeKind,
    on_embedding_progress: &dyn Fn(usize, usize),
) -> Result<(), String> {
    // Same guard `embedding_sync` acquires first, before anything else —
    // see `EmbeddingSyncGuard`'s doc comment.
    let _guard = lock_sync_guard(sync_guard);

    let relative =
        crate::domain::paths::relative_to_lenient(index_root, &path).map_err(|e| e.to_string())?;
    let file_id = FileId(relative);

    // Rename-vs-delete disambiguation lives here, not in `IndexWatcher` —
    // mirrors how `WorkspaceIndex::update_document` does its own
    // `path.exists()` check rather than `FileWatcher` doing it.
    let effective_kind = if kind == FileChangeKind::Upserted && !path.exists() {
        FileChangeKind::Removed
    } else {
        kind
    };

    if effective_kind == FileChangeKind::Upserted
        && repo_index.get(&file_id).is_none()
        && !workspace_scanner::is_new_file_indexable(index_root, &path)
    {
        return Ok(());
    }

    // Every branch below either updates the index and falls through, or
    // returns early (nothing to reconcile) — reaching past this `match`
    // always means something in `ChunkIndex`/`IndexStore` actually changed.
    match effective_kind {
        FileChangeKind::Removed => {
            repo_index.remove_file(&file_id);
            chunk_index.replace_for_file(&file_id, Vec::new());
            store.delete_files(&[file_id.clone()]).map_err(|e| e.to_string())?;
        }
        FileChangeKind::Upserted => match repo_index.update_file(&file_id) {
            Ok(()) => {
                let Some(indexed) = repo_index.get(&file_id) else {
                    return Ok(());
                };
                store
                    .upsert_files(&[indexed.metadata.clone()])
                    .map_err(|e| e.to_string())?;

                let unchanged = chunk_index
                    .file_hash_for(&file_id)
                    .is_some_and(|hash| hash == indexed.metadata.hash);
                if !unchanged {
                    let chunks = ChunkBuilder::new()
                        .build_file(repo_index, &file_id, &ChunkBuildOptions::default())
                        .map_err(|e| e.to_string())?;
                    let metadatas: Vec<_> = chunks.iter().map(|c| c.metadata.clone()).collect();
                    chunk_index.replace_for_file(&file_id, chunks);
                    store
                        .replace_chunks_for_file(&file_id, &metadatas)
                        .map_err(|e| e.to_string())?;
                    store
                        .replace_symbols_for_file(&file_id, &indexed.symbols)
                        .map_err(|e| e.to_string())?;
                    store
                        .replace_imports_for_file(&file_id, &indexed.imports)
                        .map_err(|e| e.to_string())?;
                }
            }
            // Raced a deletion after the exists() check above — treat it
            // the same as a genuine Removed event.
            Err(RepoIndexError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                repo_index.remove_file(&file_id);
                chunk_index.replace_for_file(&file_id, Vec::new());
                store.delete_files(&[file_id.clone()]).map_err(|e| e.to_string())?;
            }
            Err(e) => return Err(e.to_string()),
        },
    }

    let config = embedding_config::resolve_embedding_config().map_err(|e| e.to_string())?;
    let dimensions = embedding_providers::expected_dimensions(&config);

    // A dimension mismatch means whatever's persisted can't be trusted for
    // this provider — only a deliberate manual sync repairs that
    // (`allow_repair: true`, `embedding_sync`); an incremental tick just
    // skips embedding reconciliation for this tick rather than risk a
    // destructive repair from a background job. The chunk/repo-index
    // updates above already landed and are valid regardless.
    let persisted_dimensions: Option<usize> = store
        .read_meta(META_EMBEDDING_DIMENSIONS)
        .map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok());
    if persisted_dimensions.is_some() && persisted_dimensions != Some(dimensions) {
        return Ok(());
    }

    let api_key = embedding_credentials_store::get_api_key();
    let provider = ensure_provider(embedding_provider, &config, api_key)?;
    let builder = EmbeddingBuilder::new(provider);

    attach_embedding_index(embedding_index, store, index_root, dimensions, false)?;
    let mut slot = embedding_index
        .lock()
        .map_err(|_| "embedding index lock poisoned".to_string())?;
    let Some((_, _, index)) = slot.as_mut() else {
        return Ok(());
    };
    index
        .sync(chunk_index, &builder, index_root, Some(store), Some(on_embedding_progress))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// How many files the background backlog task chunks+embeds per
/// `EmbeddingSyncGuard` acquisition — small enough that a concurrent manual
/// sync or incremental tick never waits long, large enough that this
/// doesn't spawn a fresh SQLite transaction/lock cycle per single file for
/// a potentially large first-sync backlog.
const BACKGROUND_BATCH_FILES: usize = 25;

/// Rebuilds chunks for whichever of `file_ids` changed (skips the rest,
/// same hash-comparison `embedding_sync`'s own bulk loop uses) and
/// reconciles `EmbeddingIndex` once for the whole batch. Caller holds
/// `EmbeddingSyncGuard` for the duration (not acquired here). Mirrors
/// `embedding_sync`'s bulk loop, not `run_incremental_sync` — only the
/// *changed* subset gets `store.upsert_files`'d, matching the bulk-sync
/// convention this backlog descends from (`run_incremental_sync`'s
/// `Upserted` branch always upserts, even on an unchanged hash, to keep
/// `mtime`/`size_bytes` fresh for a touched-but-unchanged file — a
/// different, event-driven concern this batch helper doesn't share).
/// A single file's `build_file` failure (e.g. deleted between
/// `repo_index.build()` and this batch reaching it) is logged and skipped,
/// same resilience policy as `ChunkBuilder::build_all` — never aborts the
/// rest of the batch.
#[allow(clippy::too_many_arguments)]
fn sync_backlog_batch(
    repo_index: &RepositoryIndex,
    chunk_index: &ChunkIndex,
    embedding_index: &EmbeddingIndexSlot,
    embedding_provider: &EmbeddingProviderSlot,
    index_root: &Path,
    store: &IndexStore,
    file_ids: &[FileId],
    on_progress: Option<&dyn Fn(usize, usize)>,
) -> Result<SyncStats, String> {
    let chunk_builder = ChunkBuilder::new();
    let options = ChunkBuildOptions::default();

    for file_id in file_ids {
        let Some(indexed) = repo_index.get(file_id) else {
            continue;
        };
        let unchanged = chunk_index
            .file_hash_for(file_id)
            .is_some_and(|hash| hash == indexed.metadata.hash);
        if unchanged {
            continue;
        }
        // `files` must gain (or already have) a row for `file_id` before
        // `chunks` can reference it — `chunks.file_id` is a foreign key
        // (`ON DELETE CASCADE`) onto `files.file_id`.
        store
            .upsert_files(&[indexed.metadata.clone()])
            .map_err(|e| e.to_string())?;
        match chunk_builder.build_file(repo_index, file_id, &options) {
            Ok(chunks) => {
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
            }
            Err(e) => eprintln!("[embedding-sync] background: skipping {}: {e}", file_id.0),
        }
    }

    let config = embedding_config::resolve_embedding_config().map_err(|e| e.to_string())?;
    let dimensions = embedding_providers::expected_dimensions(&config);

    // Mirrors `run_incremental_sync`'s same guard: a dimension mismatch
    // means whatever's persisted can't be trusted for this provider, and
    // only a deliberate manual sync (`allow_repair: true`) repairs that —
    // this background batch just skips embedding reconciliation for now.
    // The chunk/repo-index updates above already landed and are valid
    // regardless.
    let persisted_dimensions: Option<usize> = store
        .read_meta(META_EMBEDDING_DIMENSIONS)
        .map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok());
    if persisted_dimensions.is_some() && persisted_dimensions != Some(dimensions) {
        return Ok(SyncStats::default());
    }

    let api_key = embedding_credentials_store::get_api_key();
    let provider = ensure_provider(embedding_provider, &config, api_key)?;
    let builder = EmbeddingBuilder::new(provider);

    attach_embedding_index(embedding_index, store, index_root, dimensions, false)?;
    let mut slot = embedding_index
        .lock()
        .map_err(|_| "embedding index lock poisoned".to_string())?;
    let Some((_, _, index)) = slot.as_mut() else {
        return Ok(SyncStats::default());
    };
    index
        .sync(chunk_index, &builder, index_root, Some(store), on_progress)
        .map_err(|e| e.to_string())
}


/// How many changed non-priority, non-doc files a sync processes inline
/// before deferring the rest to the background backlog — same rationale as
/// `BACKGROUND_BATCH_FILES`: small enough that splitting wouldn't have
/// helped (today's common routine-sync case), large enough that a real bulk
/// change (a big `git pull`, a fresh project's first sync) doesn't block the
/// caller. See `split_sync_tiers`.
const INLINE_TIER2_FILE_LIMIT: usize = 25;

/// Splits `current_ids` into what `embedding_sync` chunks+embeds
/// synchronously this call vs. what it defers to the background backlog.
/// Two things always land in the synchronous set: files under `docs_prefix`
/// (`project.docs_root`, repo-relative — documentation changes are meant to
/// be searchable immediately, on every sync, not just the first) and
/// `priority_ids` (open editor tabs + their direct deps, only ever
/// non-empty on a first sync — see `PriorityFilesSlot`). Everything else
/// *also* stays synchronous unless enough of it actually changed
/// (`INLINE_TIER2_FILE_LIMIT`) to be worth deferring — checked via the same
/// `chunk_index.file_hash_for(id) == repo_index.get(id).hash` comparison
/// the synchronous chunking loop below already makes, so an unchanged file
/// never counts against the limit.
fn split_sync_tiers(
    current_ids: &[FileId],
    chunk_index: &ChunkIndex,
    repo_index: &RepositoryIndex,
    docs_prefix: &str,
    priority_ids: &HashSet<FileId>,
) -> (Vec<FileId>, Vec<FileId>) {
    // "." (and, defensively, "") both mean "docs_root is the repo root" —
    // `paths::is_under_relative_prefix` only recognizes an actual nested
    // prefix, so that case is handled here rather than inside it.
    let in_docs = |id: &FileId| {
        docs_prefix.is_empty()
            || docs_prefix == "."
            || paths::is_under_relative_prefix(&id.0, docs_prefix)
    };
    let is_changed = |id: &FileId| {
        repo_index.get(id).is_some_and(|indexed| {
            !chunk_index
                .file_hash_for(id)
                .is_some_and(|hash| hash == indexed.metadata.hash)
        })
    };

    let mut sync_ids = Vec::new();
    let mut rest = Vec::new();
    for id in current_ids {
        if priority_ids.contains(id) || in_docs(id) {
            sync_ids.push(id.clone());
        } else {
            rest.push(id.clone());
        }
    }

    let changed_rest_count = rest.iter().filter(|id| is_changed(id)).count();
    if changed_rest_count <= INLINE_TIER2_FILE_LIMIT {
        sync_ids.extend(rest);
        (sync_ids, Vec::new())
    } else {
        (sync_ids, rest)
    }
}

/// Merges `new_ids` into `slot`'s queue for `index_root`, replacing
/// whatever was there if it belonged to a different `index_root` (a project
/// switch — the old entry's own drain loop notices the mismatch on its next
/// iteration and stops itself, see `run_background_backlog_sync`), and
/// returns whether the caller should spawn a fresh drain task: only when no
/// drain loop is currently claiming this queue (`running` was `false`).
/// This is the single-flight guard — without it, a routine sync's small
/// newly-discovered backlog could spawn a second, independent drain loop
/// while an earlier one (e.g. a fresh project's first sync) is still
/// working through `pending`.
fn merge_background_backlog(
    slot: &BackgroundBacklogSlot,
    index_root: &Path,
    new_ids: Vec<FileId>,
) -> Result<bool, String> {
    let mut guard = slot
        .lock()
        .map_err(|_| "background backlog lock poisoned".to_string())?;
    let is_same_root = matches!(guard.as_ref(), Some(b) if b.index_root == index_root);
    if !is_same_root {
        *guard = Some(BackgroundBacklog {
            index_root: index_root.to_path_buf(),
            pending: HashSet::new(),
            running: false,
        });
    }
    let backlog = guard.as_mut().expect("just set above if missing");
    backlog.pending.extend(new_ids);
    if backlog.running {
        Ok(false)
    } else {
        backlog.running = true;
        Ok(true)
    }
}

/// Background-eligible files (see `split_sync_tiers`), processed after
/// `embedding_sync`'s synchronous tier already returned to the caller. Runs
/// entirely on its own `spawn_blocking` task — nothing awaits this,
/// mirroring how `IndexWatcher`'s own dispatcher spawns a further
/// `spawn_blocking` per dispatched event. Loops draining
/// `background_backlog` (rather than a fixed snapshot) so a later sync's
/// newly-discovered backlog can merge into an already-running drain instead
/// of needing its own task (see `merge_background_backlog`) — acquiring
/// `EmbeddingSyncGuard` only for the duration of each batch (not once for
/// the whole backlog), so a manual "Синхронизировать" click or an
/// incremental fs-tick can interleave with this catch-up rather than wait
/// it out entirely.
#[allow(clippy::too_many_arguments)]
fn run_background_backlog_sync(
    repo_index: Arc<RepositoryIndex>,
    chunk_index: Arc<ChunkIndex>,
    embedding_index: Arc<EmbeddingIndexSlot>,
    embedding_provider: Arc<EmbeddingProviderSlot>,
    sync_guard: Arc<EmbeddingSyncGuard>,
    store: Arc<IndexStore>,
    index_root: PathBuf,
    app: AppHandle,
    background_backlog: Arc<BackgroundBacklogSlot>,
) {
    let mut done = 0usize;
    loop {
        if !is_current_index_root(&index_root) {
            eprintln!(
                "[embedding-sync] background backlog abandoned — project/index_root changed"
            );
            // Only reset the slot if it's still ours to reset — a
            // subsequent sync for a different project may have already
            // replaced it with its own fresh entry (see
            // `merge_background_backlog`), which must not be touched here.
            if let Ok(mut guard) = background_backlog.lock() {
                if matches!(guard.as_ref(), Some(b) if b.index_root == index_root) {
                    *guard = None;
                }
            }
            return;
        }

        let (batch, total_hint) = {
            let Ok(mut guard) = background_backlog.lock() else {
                eprintln!("[embedding-sync] background backlog: lock poisoned");
                return;
            };
            let Some(backlog) = guard.as_mut() else {
                return;
            };
            if backlog.index_root != index_root {
                // A different project's sync has claimed this slot for
                // itself since we started — that entry's own drain loop
                // owns it now, nothing left for this one to do.
                return;
            }
            if backlog.pending.is_empty() {
                backlog.running = false;
                return;
            }
            let batch: Vec<FileId> =
                backlog.pending.iter().take(BACKGROUND_BATCH_FILES).cloned().collect();
            for id in &batch {
                backlog.pending.remove(id);
            }
            let total_hint = done + batch.len() + backlog.pending.len();
            (batch, total_hint)
        };

        let guard = lock_sync_guard(&sync_guard);
        let on_progress = |current: usize, total_pending: usize| {
            emit_sync_progress(&app, SyncPhase::Embedding, current, total_pending, SyncTrigger::Background);
        };
        if let Err(e) = sync_backlog_batch(
            &repo_index,
            &chunk_index,
            &embedding_index,
            &embedding_provider,
            &index_root,
            &store,
            &batch,
            Some(&on_progress),
        ) {
            eprintln!("[embedding-sync] background backlog batch failed: {e}");
        }
        drop(guard);

        done += batch.len();
        emit_sync_progress(&app, SyncPhase::Chunking, done, total_hint, SyncTrigger::Background);
    }
}

/// Starts (or restarts, on an `index_root` change) the file-watcher-driven
/// incremental sync for `index_root`. Called from both `embedding_sync` and
/// `embedding_index_status` — the latter runs eagerly at project open
/// (`useEmbeddingIndexWarmup`), so incremental watching begins immediately
/// rather than waiting for the user's first manual sync. Started
/// regardless of whether the store is `stale` — `run_incremental_sync`
/// itself is a no-op until `RepositoryIndex` has a baseline (see its docs).
#[allow(clippy::too_many_arguments)]
fn ensure_incremental_watcher(
    watcher_slot: &IndexWatcherSlot,
    app: &AppHandle,
    index_root: &Path,
    store: &Arc<IndexStore>,
    repo_index: &Arc<RepositoryIndex>,
    chunk_index: &Arc<ChunkIndex>,
    embedding_index: &Arc<EmbeddingIndexSlot>,
    embedding_provider: &Arc<EmbeddingProviderSlot>,
    sync_guard: &Arc<EmbeddingSyncGuard>,
) -> Result<(), String> {
    let mut guard = watcher_slot
        .lock()
        .map_err(|_| "index watcher lock poisoned".to_string())?;
    let needs_restart = !matches!(guard.as_ref(), Some((root, _)) if root == index_root);
    if needs_restart {
        let app = app.clone();
        let store = store.clone();
        let repo_index = repo_index.clone();
        let chunk_index = chunk_index.clone();
        let embedding_index = embedding_index.clone();
        let embedding_provider = embedding_provider.clone();
        let sync_guard = sync_guard.clone();
        let index_root_owned = index_root.to_path_buf();

        let watcher = IndexWatcher::start(
            index_root.to_path_buf(),
            Duration::from_millis(400),
            |path| detect_language(&path.to_string_lossy()).is_some(),
            move |path, kind| {
                let on_progress = |current: usize, total: usize| {
                    emit_sync_progress(
                        &app,
                        SyncPhase::Embedding,
                        current,
                        total,
                        SyncTrigger::Incremental,
                    );
                };
                if let Err(e) = run_incremental_sync(
                    &repo_index,
                    &chunk_index,
                    &embedding_index,
                    &embedding_provider,
                    &sync_guard,
                    &index_root_owned,
                    &store,
                    path,
                    kind,
                    &on_progress,
                ) {
                    eprintln!("[embedding-watch] incremental sync tick failed: {e}");
                }
            },
        )
        .map_err(|e| e.to_string())?;
        *guard = Some((index_root.to_path_buf(), watcher));
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
            &app,
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
            emit_sync_progress(&app, SyncPhase::Chunking, chunked_so_far, total_changed, SyncTrigger::Full);
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
                emit_sync_progress(&app, SyncPhase::Embedding, current, total, SyncTrigger::Full);
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
                let app = app.clone();
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
                        app,
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
            &app,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::embedding_state::tests::{fixture_dir, mock_provider_slot};

    // --- `direct_dependencies` ---

    #[test]
    fn direct_dependencies_is_empty_for_an_unknown_document() {
        // Graceful degradation: a file `WorkspaceIndex` hasn't parsed this
        // session (not built yet, or a non-AsciiDoc file) simply expands to
        // itself, not an error.
        let workspace_index =
            WorkspaceIndex::new(crate::infra::parsers::registry::ParserRegistry::new());
        let file_id = FileId("guide.adoc".to_string());
        assert!(direct_dependencies(&workspace_index, &file_id).is_empty());
    }

    // --- `load_persisted_symbols` ---

    #[test]
    fn load_persisted_symbols_carries_persisted_imports_forward() {
        use crate::domain::repo_index::{FileMetadata, ImportRef, Language};
        use crate::infra::index_store::IndexStore;

        let dir = fixture_dir("load-persisted-symbols");
        let store = IndexStore::open(&dir).unwrap();

        let file_id = FileId("com/foo/Bar.java".to_string());
        store
            .upsert_files(&[FileMetadata {
                relative_path: file_id.0.clone(),
                size_bytes: 10,
                modified_at: SystemTime::now(),
                hash: blake3::hash(b"x"),
                language: Language::Java,
            }])
            .unwrap();
        let imports = vec![ImportRef { fqn: "com.foo.Baz".to_string(), is_wildcard: false }];
        store.replace_imports_for_file(&file_id, &imports).unwrap();

        let persisted = load_persisted_symbols(&store).unwrap();
        let entry = persisted.get(&file_id).unwrap();
        assert_eq!(entry.imports, imports);

        std::fs::remove_dir_all(&dir).ok();
    }

    // --- `sync_backlog_batch` ---

    use crate::domain::embeddings::{Embedding, EmbeddingError};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn sync_backlog_batch_only_touches_the_given_file_ids() {
        let root = fixture_dir("repo");
        fs::write(root.join("a.json"), "0123456789").unwrap();
        fs::write(root.join("b.json"), "abcdefghij").unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let chunk_index = ChunkIndex::new();
        let embedding_index = EmbeddingIndexSlot::new(None);
        let embedding_provider = mock_provider_slot();

        let store_dir = fixture_dir("store");
        let store = IndexStore::open(&store_dir).unwrap();

        let batch = vec![FileId("a.json".to_string())];
        let stats = sync_backlog_batch(
            &repo_index,
            &chunk_index,
            &embedding_index,
            &embedding_provider,
            &root,
            &store,
            &batch,
            None,
        )
        .unwrap();

        assert_eq!(stats.embedded, 1);
        assert!(chunk_index.file_ids().contains(&FileId("a.json".to_string())));
        assert!(!chunk_index.file_ids().contains(&FileId("b.json".to_string())));

        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&store_dir).ok();
    }

    #[test]
    fn sync_backlog_batch_skips_unchanged_files_on_a_second_pass() {
        let root = fixture_dir("repo");
        fs::write(root.join("a.json"), "0123456789").unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let chunk_index = ChunkIndex::new();
        let embedding_index = EmbeddingIndexSlot::new(None);
        let embedding_provider = mock_provider_slot();

        let store_dir = fixture_dir("store");
        let store = IndexStore::open(&store_dir).unwrap();

        let batch = vec![FileId("a.json".to_string())];
        sync_backlog_batch(
            &repo_index,
            &chunk_index,
            &embedding_index,
            &embedding_provider,
            &root,
            &store,
            &batch,
            None,
        )
        .unwrap();

        // Same batch again, nothing changed on disk in between.
        let stats = sync_backlog_batch(
            &repo_index,
            &chunk_index,
            &embedding_index,
            &embedding_provider,
            &root,
            &store,
            &batch,
            None,
        )
        .unwrap();

        assert_eq!(stats.embedded, 0);
        assert_eq!(stats.skipped_unchanged, 1);

        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&store_dir).ok();
    }

    // --- `run_incremental_sync` ---

    #[test]
    fn run_incremental_sync_indexes_a_brand_new_untracked_file() {
        let root = fixture_dir("repo");
        fs::write(root.join("existing.json"), "1").unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let chunk_index = ChunkIndex::new();
        let embedding_index = EmbeddingIndexSlot::new(None);
        let embedding_provider = mock_provider_slot();
        let sync_guard = EmbeddingSyncGuard::new(());
        let store_dir = fixture_dir("store");
        let store = IndexStore::open(&store_dir).unwrap();

        // `new.json` never went through `repo_index.build()` above — the
        // exact "watcher saw a Create for a path RepositoryIndex has never
        // heard of" scenario this fix targets.
        let new_path = root.join("new.json");
        fs::write(&new_path, "2").unwrap();
        assert!(repo_index.get(&FileId("new.json".to_string())).is_none());

        run_incremental_sync(
            &repo_index,
            &chunk_index,
            &embedding_index,
            &embedding_provider,
            &sync_guard,
            &root,
            &store,
            new_path,
            FileChangeKind::Upserted,
            &|_, _| {},
        )
        .unwrap();

        assert!(repo_index.get(&FileId("new.json".to_string())).is_some());
        assert!(chunk_index.file_ids().contains(&FileId("new.json".to_string())));

        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&store_dir).ok();
    }

    #[test]
    fn run_incremental_sync_ignores_a_new_gitignored_file() {
        let root = fixture_dir("repo");
        git2::Repository::init(&root).unwrap();
        fs::write(root.join(".gitignore"), "ignored.json\n").unwrap();
        fs::write(root.join("existing.json"), "1").unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let chunk_index = ChunkIndex::new();
        let embedding_index = EmbeddingIndexSlot::new(None);
        let embedding_provider = mock_provider_slot();
        let sync_guard = EmbeddingSyncGuard::new(());
        let store_dir = fixture_dir("store");
        let store = IndexStore::open(&store_dir).unwrap();

        let new_path = root.join("ignored.json");
        fs::write(&new_path, "2").unwrap();

        run_incremental_sync(
            &repo_index,
            &chunk_index,
            &embedding_index,
            &embedding_provider,
            &sync_guard,
            &root,
            &store,
            new_path,
            FileChangeKind::Upserted,
            &|_, _| {},
        )
        .unwrap();

        assert!(repo_index.get(&FileId("ignored.json".to_string())).is_none());
        assert!(!chunk_index.file_ids().contains(&FileId("ignored.json".to_string())));

        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&store_dir).ok();
    }

    // --- `split_sync_tiers` ---

    #[test]
    fn split_sync_tiers_puts_doc_changes_in_the_synchronous_tier_on_a_routine_sync() {
        let root = fixture_dir("repo");
        fs::write(root.join("guide.json"), "1").unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/intro.json"), "1").unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        // Nothing chunked yet in `chunk_index` — mirrors a routine sync
        // where both files are freshly-changed, not a first sync (that
        // distinction only matters for `priority_ids`, computed
        // separately in `embedding_sync`; `split_sync_tiers` itself
        // doesn't know or care whether this is a first sync).
        let chunk_index = ChunkIndex::new();

        let current_ids = repo_index.file_ids();
        let (tier1, tier2) =
            split_sync_tiers(&current_ids, &chunk_index, &repo_index, "docs", &HashSet::new());

        assert!(tier1.contains(&FileId("docs/intro.json".to_string())));
        assert!(tier2.is_empty(), "small change set folds into tier1 regardless of docs prefix");
        // Both land in tier1 here since the non-doc change set is small
        // (`INLINE_TIER2_FILE_LIMIT`) — see the large-change-set test below
        // for the case where a non-doc file is actually deferred.
        assert!(tier1.contains(&FileId("guide.json".to_string())));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn split_sync_tiers_defers_a_large_non_doc_change_set_to_the_background() {
        let root = fixture_dir("repo");
        std::fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/intro.json"), "1").unwrap();
        for i in 0..(INLINE_TIER2_FILE_LIMIT + 5) {
            fs::write(root.join(format!("f{i}.json")), "1").unwrap();
        }

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let chunk_index = ChunkIndex::new();

        let current_ids = repo_index.file_ids();
        let (tier1, tier2) =
            split_sync_tiers(&current_ids, &chunk_index, &repo_index, "docs", &HashSet::new());

        assert!(tier1.contains(&FileId("docs/intro.json".to_string())), "docs always sync");
        assert_eq!(tier2.len(), INLINE_TIER2_FILE_LIMIT + 5);
        assert!(tier2.iter().all(|id| !id.0.starts_with("docs/")));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn split_sync_tiers_does_not_count_unchanged_files_toward_the_inline_limit() {
        let root = fixture_dir("repo");
        for i in 0..(INLINE_TIER2_FILE_LIMIT + 5) {
            fs::write(root.join(format!("f{i}.json")), "1").unwrap();
        }

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        // Chunk every file up front so `split_sync_tiers` sees them all as
        // unchanged (matching hash) — none of them should count against
        // `INLINE_TIER2_FILE_LIMIT`, so everything stays synchronous.
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(ChunkBuilder::new().build_all(&repo_index, &ChunkBuildOptions::default()));

        let current_ids = repo_index.file_ids();
        let (tier1, tier2) =
            split_sync_tiers(&current_ids, &chunk_index, &repo_index, "docs", &HashSet::new());

        assert_eq!(tier1.len(), INLINE_TIER2_FILE_LIMIT + 5);
        assert!(tier2.is_empty());

        fs::remove_dir_all(&root).ok();
    }

    // --- `merge_background_backlog` ---

    #[test]
    fn merge_background_backlog_requests_a_spawn_only_when_not_already_running() {
        let slot = BackgroundBacklogSlot::new(None);
        let root = PathBuf::from("/repo");

        let first = merge_background_backlog(&slot, &root, vec![FileId("a.json".to_string())]).unwrap();
        assert!(first, "nothing running yet — must request a spawn");

        let second = merge_background_backlog(&slot, &root, vec![FileId("b.json".to_string())]).unwrap();
        assert!(!second, "a drain is already claimed — must merge without spawning again");

        let pending = slot.lock().unwrap().as_ref().unwrap().pending.len();
        assert_eq!(pending, 2, "both merges' ids must be present in the queue");
    }

    #[test]
    fn merge_background_backlog_resets_for_a_different_index_root() {
        let slot = BackgroundBacklogSlot::new(None);
        let root_a = PathBuf::from("/repo-a");
        let root_b = PathBuf::from("/repo-b");

        merge_background_backlog(&slot, &root_a, vec![FileId("a.json".to_string())]).unwrap();
        let should_spawn = merge_background_backlog(&slot, &root_b, vec![FileId("b.json".to_string())]).unwrap();

        assert!(should_spawn, "a different index_root must not inherit the stale running flag");
        let guard = slot.lock().unwrap();
        let backlog = guard.as_ref().unwrap();
        assert_eq!(backlog.index_root, root_b);
        assert_eq!(backlog.pending, HashSet::from([FileId("b.json".to_string())]));
    }

    // --- `attach_index_store` / project-switch safety ---
}
