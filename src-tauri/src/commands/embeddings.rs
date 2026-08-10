use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter, State};

use crate::domain::chunk_index::ChunkBuildOptions;
use crate::domain::embeddings::{
    EmbeddingIndexStatus, EmbeddingProvider, EmbeddingProviderConfig, ModelStatus, SyncStats,
};
use crate::domain::paths;
use crate::domain::project_config::OpenedProject;
use crate::domain::repo_index::{detect_language, FileId, RepoIndexError};
use crate::domain::workspace_index::DocumentId;
use crate::infra::index_store::IndexStore;
use crate::infra::{
    embedding_credentials_store, embedding_providers, project_store, repository_identity,
    settings_store, workspace_scanner,
};
use crate::services::chunk_builder::{ChunkBuilder, ChunkIndex};
use crate::services::embedding_config;
use crate::services::embedding_index::{EmbeddingBuilder, EmbeddingIndex};
use crate::services::embedding_model::{self, DownloadState};
use crate::services::index_store_ensure;
use crate::services::index_watcher::{FileChangeKind, IndexWatcher};
use crate::services::project_open;
use crate::services::repo_index::{RepositoryIndex, ReusableFileData};
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
/// project opened, or the provider's dimension count changed) invalidates
/// the resident index the same way. `AiAccessMode` no longer affects
/// `index_root`, so switching it is a no-op here.
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

pub(crate) fn ensure_provider(
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

/// Every `.lock()` on `EmbeddingSyncGuard` goes through this rather than
/// propagating `PoisonError` as a hard failure. The guarded value is `()`
/// — there is no actual data a panic mid-hold could leave inconsistent,
/// only the mutual-exclusion property itself, which is still perfectly
/// intact after a panic unwinds. Without this, a single panic anywhere
/// between acquiring and releasing this lock (e.g. a language-indexer bug
/// producing a malformed chunk span, as `services::chunk_builder` guards
/// against independently) would poison it *permanently* — every later
/// `embedding_sync` call, incremental-watcher tick, and semantic-search
/// readiness check would fail with "poisoned" for the rest of the app's
/// lifetime, with no recovery short of restarting the app.
pub(crate) fn lock_sync_guard(guard: &EmbeddingSyncGuard) -> std::sync::MutexGuard<'_, ()> {
    guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The running file-watcher-driven incremental sync for `index_root`, if
/// one is active. Restarted (drop old, start new) whenever `index_root`
/// changes — a project switch — via `ensure_incremental_watcher`.
/// `AiAccessMode` no longer affects `index_root` (see `resolve_index_paths`),
/// so switching it no longer restarts this.
pub type IndexWatcherSlot = Mutex<Option<(PathBuf, IndexWatcher)>>;

/// Open-editor-tab hint for a fresh project's first `embedding_sync` (see
/// its first-sync branch) — `FileId`s relative to whatever `index_root` was
/// active the last time `embedding_set_priority_files` ran. Purely
/// advisory and read only once, near the top of that branch: a
/// stale-by-one-call snapshot is harmless (the next `embedding_set_priority_files`
/// call supersedes it, and worst case a stale snapshot just fails to match
/// anything in `current_set`, falling back to today's untiered behavior).
pub type PriorityFilesSlot = Mutex<HashSet<FileId>>;

/// Background-eligible `FileId`s (see `split_sync_tiers`) queued for
/// `run_background_backlog_sync`, merged across however many `embedding_sync`
/// calls contribute to it, plus the single-flight guard (`running`) that
/// stops a routine sync's newly-discovered backlog from spawning a second,
/// independent drain loop while an earlier one (e.g. a fresh project's first
/// sync) is still working through `pending`. Keyed by `index_root`: a sync
/// for a *different* project than whatever this slot currently holds
/// replaces it outright rather than merging into stale cross-project data —
/// the old entry's own drain loop notices the mismatch on its next
/// iteration (see `run_background_backlog_sync`) and stops itself without
/// touching the new one.
pub struct BackgroundBacklog {
    index_root: PathBuf,
    pending: HashSet<FileId>,
    running: bool,
}
pub type BackgroundBacklogSlot = Mutex<Option<BackgroundBacklog>>;

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

/// Resolves both paths a project's index needs. `index_root` is always
/// `project.root` — the index covers the whole repository unconditionally
/// now, `AiAccessMode` no longer selects a subtree to walk (see
/// `domain::ai_tools::ToolScope` for how the `DocsOnly` boundary is
/// preserved instead, at query time rather than by physically indexing
/// less). This is what `RepositoryIndex`/`ChunkBuilder`/
/// `chunk_text::resolve_text` resolve relative `FileId`s against, and what
/// keys the `ChunkIndex`/`EmbeddingIndexSlot` attach state.
///
/// `storage_dir` is a **global**, per-repository location —
/// `~/.atlas/embeddings/{repository_id}/` — not inside the repo at all.
/// `repository_id` is `repository_identity::repository_id` of the repo's
/// canonical remote URL (`infra::repository_identity::resolve`), or, for a
/// repo with no resolvable remote (not a git repo, or a git repo with no
/// remotes), a per-project UUID persisted in `{project.root}/.atlas/
/// project.json` the first time it's needed (`local_identity`). Keying by
/// repository identity rather than by `index_root` means the same repo
/// resolves to the same cache regardless of which local path it's cloned
/// or worktree-checked-out to — `index_store_ensure::open_for` no longer
/// treats a different `index_root` as staleness for exactly this reason.
/// The revision (`RepositoryIdentity::revision`) is deliberately *not*
/// part of `repository_id`: baking it in would turn every commit into a
/// brand-new, empty cache folder. It's instead recorded as informational
/// metadata by `index_store_ensure::repair_stale`.
pub(crate) fn resolve_index_paths(project: &OpenedProject) -> Result<(PathBuf, PathBuf), String> {
    let index_root = PathBuf::from(&project.root);
    let identity = repository_identity::resolve(&index_root);
    let source = match identity.canonical_url {
        Some(url) => url,
        None => local_identity(&project.root)?,
    };
    let repository_id = repository_identity::repository_id(&source);
    let storage_dir = settings_store::settings_dir()
        .map_err(|e| e.to_string())?
        .join("embeddings")
        .join(repository_id);
    Ok((index_root, storage_dir))
}

/// Fallback identity for a repo with no resolvable canonical remote URL —
/// a random UUID, generated once and persisted to `{repo_root}/.atlas/
/// project.json` so the same project keeps resolving to the same global
/// embeddings folder across sessions. `project.json` is assumed to already
/// exist: every caller of `resolve_index_paths` runs only after
/// `project_open::open_project`, which always creates it first.
fn local_identity(repo_root: &str) -> Result<String, String> {
    let mut config = project_store::load(repo_root)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no project.json found for {repo_root}"))?;

    if let Some(id) = config.local_repository_id.clone() {
        return Ok(id);
    }

    let id = uuid::Uuid::new_v4().to_string();
    config.local_repository_id = Some(id.clone());
    project_store::save(repo_root, &config).map_err(|e| e.to_string())?;
    Ok(id)
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
/// describe an incompatible chunking algorithm — the caller decides what
/// to do with a stale attach (`embedding_sync` repairs it;
/// `embedding_index_status` just reports it).
pub(crate) fn attach_index_store(
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
        let attachment = index_store_ensure::open_for(storage_dir)?;
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
pub(crate) fn attach_embedding_index(
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

/// Combines `store`'s persisted per-file metadata (content hash, size,
/// mtime, language) with its persisted symbols into the shape
/// `RepositoryIndex::build_reusing_symbols` wants — what a fresh (e.g.
/// just-restarted) `embedding_sync` call feeds it so a file whose mtime/size
/// (cheapest check) or content hash (fallback) still match the last sync
/// skips a tree-sitter/pulldown-cmark re-parse entirely, not just
/// re-embedding. `imports` is deliberately always empty here — Java imports
/// aren't persisted to SQLite (see `ReusableFileData`'s doc comment); a
/// reused entry's imports go stale until that file is genuinely re-parsed,
/// which is fine only because nothing reads `imports` outside a *true* first
/// sync, when `persisted`/`resident` are both empty by construction anyway.
fn load_persisted_symbols(store: &IndexStore) -> Result<HashMap<FileId, ReusableFileData>, String> {
    let files = store.load_all_files().map_err(|e| e.to_string())?;
    let mut symbols_by_file = store.load_all_symbols().map_err(|e| e.to_string())?;
    Ok(files
        .into_iter()
        .map(|(file_id, metadata)| {
            let symbols = symbols_by_file.remove(&file_id).unwrap_or_default();
            (file_id, ReusableFileData { metadata, symbols, imports: Vec::new() })
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
                store
                    .replace_symbols_for_file(file_id, &indexed.symbols)
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

    tauri::async_runtime::spawn_blocking(move || -> Result<SyncStats, String> {
        // Acquired first, before any other slot, and held for the entire
        // pipeline — see `EmbeddingSyncGuard`'s doc comment for why a full
        // sync and an incremental tick must never interleave.
        let _guard = lock_sync_guard(&sync_guard);

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

        let persisted_symbols = load_persisted_symbols(&store)?;
        repo_index
            .build_reusing_symbols(&index_root, &persisted_symbols)
            .map_err(|e| e.to_string())?;

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

    // --- `resolve_index_paths` ---

    #[test]
    fn resolve_index_paths_always_indexes_the_whole_repo() {
        use crate::domain::project_config::ProjectConfig;
        use crate::infra::settings_store::test_support::with_temp_home;
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        // `resolve_index_paths` now resolves through `settings_store::
        // settings_dir()` (global storage), which reads the process-global
        // `$HOME` — isolate this test's view of it, same as `chat_store`'s
        // tests, so it can't race other tests that do the same.
        with_temp_home(|| {
            static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("alfa-atlas-resolve-index-paths-{nanos}-{n}"));
            std::fs::create_dir_all(&root).unwrap();
            // `resolve_index_paths` only ever runs after `open_project`, which
            // always creates `project.json` first — set that up so the
            // no-git-remote fallback path (`local_identity`) has something to
            // read/persist into, same as in the real flow.
            project_store::save(&root.to_string_lossy(), &ProjectConfig::new(".")).unwrap();

            let project = OpenedProject {
                root: root.to_string_lossy().into_owned(),
                docs_root: root.to_string_lossy().into_owned(),
            };
            let (index_root, storage_dir) = resolve_index_paths(&project).unwrap();
            assert_eq!(index_root, root);

            // `root` isn't a git repo, so this exercises the fallback UUID
            // identity — the storage dir must be global (`~/.atlas/embeddings/
            // {64-hex-char sha256}`), never under the project root anymore.
            let embeddings_root = settings_store::settings_dir().unwrap().join("embeddings");
            assert!(storage_dir.starts_with(&embeddings_root));
            let repository_id = storage_dir.strip_prefix(&embeddings_root).unwrap();
            let repository_id = repository_id.to_string_lossy();
            assert_eq!(repository_id.len(), 64);
            assert!(repository_id.chars().all(|c| c.is_ascii_hexdigit()));

            // Resolving again must yield the same folder — the fallback UUID
            // just got persisted into `project.json`, so it's stable now.
            let (_, storage_dir_again) = resolve_index_paths(&project).unwrap();
            assert_eq!(storage_dir, storage_dir_again);

            std::fs::remove_dir_all(&root).ok();
        });
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
        assert!(direct_dependencies(&workspace_index, &file_id).is_empty());
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
}
