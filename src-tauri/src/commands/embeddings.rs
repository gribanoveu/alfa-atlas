use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter, State};

use crate::domain::ai_access::AiAccessMode;
use crate::domain::chunk_index::ChunkBuildOptions;
use crate::domain::embeddings::{
    EmbeddingIndexStatus, EmbeddingProvider, EmbeddingProviderConfig, ModelStatus, SyncStats,
};
use crate::domain::paths;
use crate::domain::project_config::{OpenedProject, ProjectConfig};
use crate::domain::repo_index::{detect_language, FileId, RepoIndexError};
use crate::domain::workspace_index::DocumentId;
use crate::infra::index_store::IndexStore;
use crate::infra::{embedding_credentials_store, embedding_providers, project_store};
use crate::services::chunk_builder::{ChunkBuilder, ChunkIndex};
use crate::services::embedding_config;
use crate::services::embedding_index::{EmbeddingBuilder, EmbeddingIndex};
use crate::services::embedding_model::{self, DownloadState};
use crate::services::index_store_ensure;
use crate::services::index_watcher::{FileChangeKind, IndexWatcher};
use crate::services::project_open;
use crate::services::repo_index::RepositoryIndex;
use crate::services::workspace_index::WorkspaceIndex;

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

/// Caches the constructed `EmbeddingProvider` across calls — for the Local
/// provider, `provider_for` constructs `LocalEmbeddingProvider::try_new()`,
/// a full ONNX model load (~570MB); doing that on every sync (and, once
/// wired up, every incremental file-watcher tick) would be unacceptable.
/// Keyed by `(config, api_key)` rather than `config` alone — a `Remote`
/// provider closes over the API key at construction time, so a key
/// rotation with an otherwise-unchanged config must still invalidate the
/// cache. Global (not per-project): the provider choice itself is global
/// (`AppSettings.embedding`), not per-repo.
pub type EmbeddingProviderSlot = Mutex<Option<(EmbeddingProviderConfig, Option<String>, Arc<dyn EmbeddingProvider>)>>;

fn ensure_provider(
    slot: &EmbeddingProviderSlot,
    config: &EmbeddingProviderConfig,
    api_key: Option<String>,
) -> Result<Arc<dyn EmbeddingProvider>, String> {
    let mut guard = slot
        .lock()
        .map_err(|_| "embedding provider lock poisoned".to_string())?;
    let stale = !matches!(guard.as_ref(), Some((c, k, _)) if c == config && *k == api_key);
    if stale {
        let provider = embedding_providers::provider_for(config, api_key.clone())
            .map_err(|e| e.to_string())?;
        *guard = Some((config.clone(), api_key, Arc::from(provider)));
    }
    Ok(guard.as_ref().expect("just set above if missing").2.clone())
}

/// Serializes every full `embedding_sync` against every incremental
/// file-watcher tick (and against each other) so the two mutation
/// pipelines — each of which reads-then-writes `RepositoryIndex`/
/// `ChunkIndex`/`IndexStore`/`EmbeddingIndex` across several non-atomic
/// steps — can never interleave. Acquired first, before any other slot,
/// and held for the whole pipeline in both `embedding_sync` and the
/// incremental path; never re-entered, never acquired while already
/// holding `IndexStoreSlot`/`EmbeddingIndexSlot`/`EmbeddingProviderSlot`.
/// A manual "Синхронизировать" click during an in-flight incremental tick
/// simply waits it out (typically sub-second) rather than racing it.
pub type EmbeddingSyncGuard = Mutex<()>;

/// The running file-watcher-driven incremental sync for `index_root`, if
/// one is active. Restarted (drop old, start new) whenever `index_root`
/// changes — a project/access-mode switch — via `ensure_incremental_watcher`.
pub type IndexWatcherSlot = Mutex<Option<(PathBuf, IndexWatcher)>>;

/// Open-editor-tab hint for a fresh project's first `embedding_sync` (see
/// its first-sync branch) — `FileId`s relative to whatever `index_root` was
/// active the last time `embedding_set_priority_files` ran. Purely
/// advisory and read only once, near the top of that branch: a
/// stale-by-one-call snapshot is harmless (the next `embedding_set_priority_files`
/// call supersedes it, and worst case a stale snapshot just fails to match
/// anything in `current_set`, falling back to today's untiered behavior —
/// see the module docs on the `AiAccessMode`-switch-without-touching-tabs
/// edge case this leaves unhandled).
pub type PriorityFilesSlot = Mutex<HashSet<FileId>>;

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

/// `index_root`'s path relative to `project.root` — empty in `FullRepo`
/// mode (`index_root == project.root`), e.g. `"src/docs/asciidoc"` in
/// `DocsOnly` mode. The pure string relationship
/// `document_id_to_file_id`/`file_id_to_document_id` bridge on, since a
/// `WorkspaceIndex::DocumentId` is always repo-root-relative while a
/// `FileId` is `index_root`-relative.
fn index_root_suffix_for(project: &OpenedProject, index_root: &Path) -> Result<String, String> {
    let repo_root = PathBuf::from(&project.root);
    let suffix = paths::relative_to(&repo_root, index_root).map_err(|e| e.to_string())?;
    Ok(if suffix == "." { String::new() } else { suffix })
}

/// A `WorkspaceIndex::find_includes`/`find_references` target — already a
/// resolved, repo-root-relative string (see `WorkspaceIndex::insert_parsed`,
/// which runs every include/xref target through `resolve_against_document`
/// before storing it) — into this sync's `FileId` space. `None` when the
/// target falls outside `index_root` (e.g. an include reaching outside
/// `docs_root` in `DocsOnly` mode — `RepositoryIndex` never walked it, so
/// there's no `FileId` for it).
fn document_id_to_file_id(repo_relative: &str, index_root_suffix: &str) -> Option<FileId> {
    if index_root_suffix.is_empty() {
        return Some(FileId(repo_relative.to_string()));
    }
    repo_relative
        .strip_prefix(index_root_suffix)
        .and_then(|s| s.strip_prefix('/'))
        .map(|s| FileId(s.to_string()))
}

/// Inverse of `document_id_to_file_id` — what `direct_dependencies` uses to
/// turn an open file's `FileId` into the `DocumentId` key
/// `WorkspaceIndex::find_includes`/`find_references` expect.
fn file_id_to_document_id(file_id: &FileId, index_root_suffix: &str) -> DocumentId {
    if index_root_suffix.is_empty() {
        DocumentId::new(file_id.0.clone())
    } else {
        DocumentId::new(format!("{index_root_suffix}/{}", file_id.0))
    }
}

/// One open file's direct (one-hop, non-transitive) AsciiDoc dependencies —
/// its `include::`/`xref:` targets. Empty for a file `WorkspaceIndex`
/// doesn't know about (not built yet this session, or a non-AsciiDoc file
/// with no include/xref syntax) — graceful degradation, not an error: the
/// priority tier then just contains the open file itself.
fn direct_dependencies(
    workspace_index: &WorkspaceIndex,
    file_id: &FileId,
    index_root_suffix: &str,
) -> Vec<FileId> {
    let doc_id = file_id_to_document_id(file_id, index_root_suffix);
    let mut out = Vec::new();
    for inc in workspace_index.find_includes(&doc_id) {
        out.extend(document_id_to_file_id(&inc.path, index_root_suffix));
    }
    for r in workspace_index.find_references(&doc_id) {
        if r.target_document.is_empty() {
            // Same-document `#anchor` xref — not a cross-file dependency.
            continue;
        }
        out.extend(document_id_to_file_id(&r.target_document, index_root_suffix));
    }
    out
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
            // `EmbeddingIndex::load`, not `::new` — the vectors file was
            // just deleted (or never existed), so `persisted_hashes` is
            // correctly empty, but `VectorStore` still needs a real path
            // remembered so `EmbeddingIndex::sync`'s `save()` actually
            // fires later. `::new` leaves `VectorStore.path` as `None`,
            // which silently makes every sync after this one skip
            // persisting to `vectors.usearch` entirely — the first sync of
            // every new project would go unsaved forever.
            EmbeddingIndex::load(dimensions, &store.vectors_path(), Vec::new())
                .map_err(|e| e.to_string())?
        } else {
            // Read-only path: report as empty for this dimension without
            // touching whatever's actually persisted on disk.
            EmbeddingIndex::new(dimensions).map_err(|e| e.to_string())?
        };
        *slot = Some((index_root.to_path_buf(), dimensions, fresh));
    }
    Ok(())
}

/// The file-watcher's `on_change` reaction — the incremental counterpart to
/// `embedding_sync`'s per-file diff loop, for exactly one changed path.
/// Called from `IndexWatcher`'s own `spawn_blocking` task (see that
/// module's docs), so this runs entirely synchronously and never blocks
/// Tauri's async runtime — matching requirement 4 (never block the UI or a
/// tool-call).
///
/// Only already-tracked files are updated incrementally (`repo_index.get`
/// must already know about `file_id`) — a genuinely new file, or one
/// that's always been gitignored, waits for the next full/manual
/// `embedding_sync`, which does the real gitignore-aware walk
/// (`workspace_scanner::scan_all`). This also means nothing happens here
/// until `RepositoryIndex` has a baseline for this `index_root` (at least
/// one `embedding_sync` this session) — expected, not a bug:
/// `RepositoryIndex` has no persistence of its own.
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
    let _guard = sync_guard
        .lock()
        .map_err(|_| "embedding sync guard poisoned".to_string())?;

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

    if effective_kind == FileChangeKind::Upserted && repo_index.get(&file_id).is_none() {
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

    let config = embedding_config::load_embedding_config().map_err(|e| e.to_string())?;
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
            }
            Err(e) => eprintln!("[embedding-sync] background: skipping {}: {e}", file_id.0),
        }
    }

    let config = embedding_config::load_embedding_config().map_err(|e| e.to_string())?;
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

/// `true` if the currently open project still resolves to `index_root` —
/// the background backlog's abort check. Unlike a single incremental tick,
/// the backlog can run long enough to plausibly outlive the project it
/// started for; continuing past a project switch would mutate
/// `EmbeddingIndexSlot`/`IndexStoreSlot` on behalf of a project that
/// already reattached them to something else.
fn is_current_index_root(index_root: &Path) -> bool {
    let Ok(Some(project)) = project_open::get_project() else {
        return false;
    };
    let Ok((root, _)) = resolve_index_paths(&project) else {
        return false;
    };
    root == index_root
}

/// The rest of a fresh project's files, processed after the first sync's
/// priority tier (open files + direct deps) already returned to the
/// caller. Runs entirely on its own `spawn_blocking` task — nothing awaits
/// this, mirroring how `IndexWatcher`'s own dispatcher spawns a further
/// `spawn_blocking` per dispatched event. Acquires `EmbeddingSyncGuard` only
/// for the duration of each batch (not once for the whole backlog), so a
/// manual "Синхронизировать" click or an incremental fs-tick can interleave
/// with this catch-up rather than wait it out entirely.
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
    backlog: Vec<FileId>,
) {
    let total = backlog.len();
    let mut done = 0usize;

    for batch in backlog.chunks(BACKGROUND_BATCH_FILES) {
        if !is_current_index_root(&index_root) {
            eprintln!(
                "[embedding-sync] background backlog abandoned — project/index_root changed"
            );
            return;
        }

        let guard = match sync_guard.lock() {
            Ok(g) => g,
            Err(_) => {
                eprintln!("[embedding-sync] background backlog: sync guard poisoned");
                return;
            }
        };
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
            batch,
            Some(&on_progress),
        ) {
            eprintln!("[embedding-sync] background backlog batch failed: {e}");
        }
        drop(guard);

        done += batch.len();
        emit_sync_progress(&app, SyncPhase::Chunking, done, total, SyncTrigger::Background);
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

    tauri::async_runtime::spawn_blocking(move || -> Result<SyncStats, String> {
        // Acquired first, before any other slot, and held for the entire
        // pipeline — see `EmbeddingSyncGuard`'s doc comment for why a full
        // sync and an incremental tick must never interleave.
        let _guard = sync_guard
            .lock()
            .map_err(|_| "embedding sync guard poisoned".to_string())?;

        let project = project_open::get_project()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no project is open".to_string())?;
        let (index_root, storage_dir) = resolve_index_paths(&project)?;
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

        repo_index.build(&index_root).map_err(|e| e.to_string())?;

        // A fresh project (nothing chunked yet, in this store or ever) is
        // the only case that gets the two-tier open-files-first treatment
        // below — a routine re-sync's changed-file count is normally small
        // enough that splitting it wouldn't help, so it keeps behaving
        // exactly as before.
        let is_first_sync = chunk_index.chunk_ids().is_empty();

        let current_ids = repo_index.file_ids();
        let current_set: HashSet<_> = current_ids.iter().cloned().collect();

        // Open editor files (plus their direct includes/xrefs, resolved via
        // `WorkspaceIndex`) get chunked+embedded first so this call returns
        // quickly with a useful partial index; the rest of a fresh
        // project's files are deferred to `run_background_backlog_sync`
        // below. Empty on anything but a first sync, and also empty
        // whenever no priority file survives the `current_set` intersection
        // (nothing open, or a stale `PriorityFilesSlot` snapshot from a
        // different `index_root` — see that type's doc comment) — either
        // way `tier1_ids` then simply becomes every file, matching today's
        // untiered behavior.
        let priority_ids: HashSet<FileId> = if is_first_sync {
            let opened = priority_files
                .lock()
                .map_err(|_| "priority files lock poisoned".to_string())?
                .clone();
            let mut set = opened.clone();
            if !opened.is_empty() {
                let suffix = index_root_suffix_for(&project, &index_root)?;
                for file_id in &opened {
                    set.extend(direct_dependencies(&workspace_index, file_id, &suffix));
                }
            }
            set.retain(|id| current_set.contains(id));
            set
        } else {
            HashSet::new()
        };
        let (tier1_ids, tier2_ids): (Vec<FileId>, Vec<FileId>) = if !priority_ids.is_empty() {
            current_ids
                .iter()
                .cloned()
                .partition(|id| priority_ids.contains(id))
        } else {
            (current_ids.clone(), Vec::new())
        };

        let chunk_builder = ChunkBuilder::new();
        let options = ChunkBuildOptions::default();

        // Only files whose content hash moved since `ChunkIndex` last saw
        // them get re-chunked — for the rest, `build_file` (and the file
        // read it requires) is skipped entirely, and `EmbeddingIndex::sync`
        // below will correctly see their chunks' hashes as unchanged
        // without this sync ever touching their text. Scoped to `tier1_ids`
        // — `tier2_ids` (empty outside a first sync) is handled by the
        // background backlog task, not here.
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
            emit_sync_progress(&app, SyncPhase::Chunking, chunked_so_far, total_changed, SyncTrigger::Full);
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
        let provider = ensure_provider(&embedding_provider, &config, api_key)?;
        let dimensions = provider.dimensions();
        let builder = EmbeddingBuilder::new(provider);

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

        // The rest of a fresh project's files, if any — dispatched to its
        // own task so this call can return `stats` (the priority tier's
        // result) now instead of waiting out the whole repo. See
        // `run_background_backlog_sync`'s doc comment for why it acquires
        // `EmbeddingSyncGuard` per batch rather than once up front.
        if is_first_sync && !tier2_ids.is_empty() {
            let repo_index = repo_index.clone();
            let chunk_index = chunk_index.clone();
            let embedding_index = embedding_index.clone();
            let embedding_provider = embedding_provider.clone();
            let sync_guard = sync_guard.clone();
            let store = store.clone();
            let index_root = index_root.clone();
            let app = app.clone();
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
                    tier2_ids,
                );
            });
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
) -> Result<EmbeddingIndexStatus, String> {
    let repo_index = repo_index.inner().clone();
    let chunk_index = chunk_index.inner().clone();
    let embedding_index = embedding_index.inner().clone();
    let index_store = index_store.inner().clone();
    let embedding_provider = embedding_provider.inner().clone();
    let sync_guard = sync_guard.inner().clone();
    let index_watcher = index_watcher.inner().clone();

    tauri::async_runtime::spawn_blocking(move || -> Result<EmbeddingIndexStatus, String> {
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

        let config = embedding_config::load_embedding_config().map_err(|e| e.to_string())?;
        let dimensions = embedding_providers::expected_dimensions(&config);

        attach_embedding_index(&embedding_index, &store, &index_root, dimensions, false)?;
        let slot = embedding_index
            .lock()
            .map_err(|_| "embedding index lock poisoned".to_string())?;
        let (_, _, index) = slot.as_ref().expect("attach_embedding_index just set this");
        let embedded_count = index.len();
        // Files the repo walk found but that haven't been chunked yet —
        // `0` outside a fresh project's first-sync backlog, since every
        // other path chunks every known file in the same pass. See
        // `EmbeddingIndexStatus::background_pending`'s doc comment.
        let background_pending = repo_index
            .file_ids()
            .len()
            .saturating_sub(chunk_index.file_ids().len());
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
    use crate::domain::embeddings::EmbeddingProviderKind;

    /// `Remote` config — cheap to construct (`RemoteEmbeddingProvider::new`
    /// does no network I/O), unlike `Local`, which would load the ONNX
    /// model.
    fn remote_config(model: &str) -> EmbeddingProviderConfig {
        EmbeddingProviderConfig {
            kind: EmbeddingProviderKind::Remote,
            remote_base_url: Some("https://api.example.com".to_string()),
            remote_model: Some(model.to_string()),
            remote_dimensions: Some(768),
        }
    }

    #[test]
    fn ensure_provider_reuses_cached_instance_when_unchanged() {
        let slot = EmbeddingProviderSlot::new(None);
        let config = remote_config("m1");

        let first = ensure_provider(&slot, &config, Some("key1".to_string())).unwrap();
        let second = ensure_provider(&slot, &config, Some("key1".to_string())).unwrap();

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn ensure_provider_rebuilds_on_config_change() {
        let slot = EmbeddingProviderSlot::new(None);

        let first = ensure_provider(&slot, &remote_config("m1"), Some("key1".to_string())).unwrap();
        let second = ensure_provider(&slot, &remote_config("m2"), Some("key1".to_string())).unwrap();

        assert!(!Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn ensure_provider_rebuilds_on_api_key_change() {
        let slot = EmbeddingProviderSlot::new(None);
        let config = remote_config("m1");

        let first = ensure_provider(&slot, &config, Some("key1".to_string())).unwrap();
        let second = ensure_provider(&slot, &config, Some("key2".to_string())).unwrap();

        assert!(!Arc::ptr_eq(&first, &second));
    }

    // --- `document_id_to_file_id` / `file_id_to_document_id` ---

    #[test]
    fn file_id_document_id_round_trip_with_empty_suffix() {
        // `FullRepo` mode: index_root == project.root, so the two spaces
        // are identical strings.
        let file_id = FileId("docs/guide.adoc".to_string());
        let doc_id = file_id_to_document_id(&file_id, "");
        assert_eq!(doc_id, DocumentId::new("docs/guide.adoc"));
        assert_eq!(document_id_to_file_id(&doc_id.0, ""), Some(file_id));
    }

    #[test]
    fn file_id_document_id_round_trip_with_a_suffix() {
        // `DocsOnly` mode: index_root == project.docs_root, a subdirectory
        // of project.root.
        let file_id = FileId("guide.adoc".to_string());
        let suffix = "src/docs/asciidoc";
        let doc_id = file_id_to_document_id(&file_id, suffix);
        assert_eq!(doc_id, DocumentId::new("src/docs/asciidoc/guide.adoc"));
        assert_eq!(document_id_to_file_id(&doc_id.0, suffix), Some(file_id));
    }

    #[test]
    fn document_id_to_file_id_is_none_outside_the_index_root() {
        // An include/xref reaching outside `docs_root` in `DocsOnly` mode —
        // `RepositoryIndex` never walked it, so there's no `FileId` for it.
        assert_eq!(
            document_id_to_file_id("src/main/java/Foo.java", "src/docs/asciidoc"),
            None
        );
    }

    // --- `direct_dependencies` ---

    #[test]
    fn direct_dependencies_is_empty_for_an_unknown_document() {
        // Graceful degradation: a file `WorkspaceIndex` hasn't parsed this
        // session (not built yet, or a non-AsciiDoc file) simply expands to
        // itself, not an error.
        let workspace_index =
            WorkspaceIndex::new(crate::infra::parsers::registry::ParserRegistry::new());
        let file_id = FileId("guide.adoc".to_string());
        assert!(direct_dependencies(&workspace_index, &file_id, "").is_empty());
    }

    // --- `sync_backlog_batch` ---

    use crate::domain::embeddings::{Embedding, EmbeddingError};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("alfa-atlas-embeddings-cmd-{label}-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Deterministic fake — never touches `fastembed`/network. Dimension is
    /// configurable so a test can match whatever `expected_dimensions`
    /// resolves to for the real config on the machine running the test.
    struct MockProvider {
        dimensions: usize,
    }

    impl EmbeddingProvider for MockProvider {
        fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError> {
            Ok(texts
                .iter()
                .map(|t| Embedding(vec![t.len() as f32; self.dimensions]))
                .collect())
        }

        fn dimensions(&self) -> usize {
            self.dimensions
        }
    }

    /// Pre-populates `EmbeddingProviderSlot` with a mock cached under
    /// whatever `embedding_config::load_embedding_config`/
    /// `embedding_credentials_store::get_api_key` actually return on this
    /// machine — `ensure_provider`'s cache check (same config, same key) then
    /// finds a hit and never calls the real `provider_for` (which would load
    /// the ~570MB local ONNX model, or fail outright, if this test's config
    /// happens to be `Local` or an incomplete `Remote` config).
    fn mock_provider_slot() -> EmbeddingProviderSlot {
        let config = embedding_config::load_embedding_config().unwrap_or_default();
        let api_key = embedding_credentials_store::get_api_key();
        let dimensions = embedding_providers::expected_dimensions(&config);
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(MockProvider { dimensions });
        EmbeddingProviderSlot::new(Some((config, api_key, provider)))
    }

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
}
