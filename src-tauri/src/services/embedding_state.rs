//! Resident, per-project state for the embeddings index — the slots Tauri
//! manages (`app.manage`) plus the attach/resolve helpers that decide which
//! project's `IndexStore`/`EmbeddingIndex` is currently loaded into them.
//!
//! Split out of `commands::embeddings` so the application layer owns this
//! state rather than the IPC boundary: `services::ai_tools`' semantic search
//! needs the very same slots and attach helpers, and reaching *up* into
//! `commands/` for them inverted the crate's dependency direction.
//!
//! Nothing here walks the repository, embeds anything, or emits progress —
//! that is `services::embedding_sync`'s job. These functions are cheap and
//! (apart from `attach_embedding_index`'s `allow_repair` path) read-only.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::domain::embeddings::{EmbeddingProvider, ResolvedEmbeddingConfig};
use crate::domain::project_config::OpenedProject;
use crate::domain::repo_index::FileId;
use crate::infra::index_store::IndexStore;
use crate::infra::{embedding_providers, project_store, repository_identity, settings_store};
use crate::services::chunk_builder::ChunkIndex;
use crate::services::embedding_index::EmbeddingIndex;
use crate::services::embedding_config;
use crate::services::index_store_ensure;
use crate::services::index_watcher::IndexWatcher;
use crate::services::project_open;
use crate::services::repo_index::RepositoryIndex;
use crate::services::workspace_index::WorkspaceIndex;

pub(crate) const META_EMBEDDING_DIMENSIONS: &str = "embedding_dimensions";


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
pub type EmbeddingProviderSlot =
    Mutex<Option<(ResolvedEmbeddingConfig, Option<String>, Arc<dyn EmbeddingProvider>)>>;

pub(crate) fn ensure_provider(
    slot: &EmbeddingProviderSlot,
    config: &ResolvedEmbeddingConfig,
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

/// Set only while a *full* `embedding_sync` walk is actually running for the
/// current project (not while incremental per-file watcher ticks run — those
/// are sub-second and safe to interleave with, see `EmbeddingSyncGuard`).
/// Checked by the checkout-family git commands to reject a branch switch
/// that would otherwise race the walk's reads against a concurrent
/// `git checkout` rewriting the working tree.
pub type FullSyncActiveSlot = std::sync::atomic::AtomicBool;

/// RAII guard that flips `FullSyncActiveSlot` true on construction and back
/// to false on drop, so it's cleared on every exit path out of the
/// `embedding_sync` closure — success, an early `?` return, or a panic
/// unwind — without needing a matching store on each branch.
pub(crate) struct FullSyncActiveGuard<'a>(&'a FullSyncActiveSlot);

impl<'a> FullSyncActiveGuard<'a> {
    pub(crate) fn new(flag: &'a FullSyncActiveSlot) -> Self {
        flag.store(true, std::sync::atomic::Ordering::Release);
        Self(flag)
    }
}

impl Drop for FullSyncActiveGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
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
    pub(crate) index_root: PathBuf,
    pub(crate) pending: HashSet<FileId>,
    pub(crate) running: bool,
}
pub type BackgroundBacklogSlot = Mutex<Option<BackgroundBacklog>>;

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
/// cleared rather than loaded from metadata that might describe an
/// incompatible chunking algorithm — the caller decides what to do with a
/// stale attach (`embedding_sync` repairs it; `embedding_index_status` just
/// reports it). Clearing (not leaving the previous project's chunks) is
/// required so a project switch onto a never-synced / version-mismatched
/// store cannot leave foreign `ChunkId`s resident for the next sync to
/// try writing into the new SQLite DB (FOREIGN KEY failures on
/// `embeddings`/`chunks`).
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
        eprintln!(
            "[embedding] attaching index store at {} (stale={})",
            storage_dir.display(),
            attachment.stale
        );
        if attachment.stale {
            chunk_index.clear();
        } else {
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
            eprintln!(
                "[embedding] loaded {} persisted embeddings ({dimensions} dims)",
                persisted_hashes.len()
            );
            EmbeddingIndex::load(dimensions, &store.vectors_path(), persisted_hashes)
                .map_err(|e| e.to_string())?
        } else if allow_repair {
            eprintln!(
                "[embedding] dimension mismatch (persisted={persisted_dimensions:?}, expected={dimensions}) — clearing and rebuilding index"
            );
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

/// `true` if the currently open project still resolves to `index_root` —
/// abort check for the background backlog and for a full `embedding_sync`.
/// Both can run long enough to outlive the project they started for;
/// continuing past a project switch would mutate shared
/// `ChunkIndex`/`EmbeddingIndexSlot`/`IndexStoreSlot` on behalf of a
/// project that is about to (or already did) reattach them to something
/// else.
pub(crate) fn is_current_index_root(index_root: &Path) -> bool {
    let Ok(Some(project)) = project_open::get_project() else {
        return false;
    };
    let Ok((root, _)) = resolve_index_paths(&project) else {
        return false;
    };
    root == index_root
}

/// Every resident slot one embeddings use-case needs, in one place.
///
/// `commands` assembles this from `tauri::State` and hands it to
/// `services::embedding_sync`, so the service signatures don't grow an
/// eleven-parameter list that has to be kept in the same order at four call
/// sites. Cheap to construct and to pass around — every field is an `Arc`.
pub struct EmbeddingSession {
    pub repo_index: Arc<RepositoryIndex>,
    pub chunk_index: Arc<ChunkIndex>,
    pub embedding_index: Arc<EmbeddingIndexSlot>,
    pub index_store: Arc<IndexStoreSlot>,
    pub embedding_provider: Arc<EmbeddingProviderSlot>,
    pub sync_guard: Arc<EmbeddingSyncGuard>,
    pub index_watcher: Arc<IndexWatcherSlot>,
    pub workspace_index: Arc<WorkspaceIndex>,
    pub priority_files: Arc<PriorityFilesSlot>,
    pub background_backlog: Arc<BackgroundBacklogSlot>,
    pub full_sync_active: Arc<FullSyncActiveSlot>,
}

/// The currently open project's index store, attached and ready to read.
pub struct AttachedIndex {
    pub index_root: PathBuf,
    pub store: Arc<IndexStore>,
    /// The persisted content predates the current chunking/index version
    /// (see `index_store_ensure`) — nothing in it is safe to read, and only
    /// a real `embedding_sync` repairs it. Reported rather than swallowed:
    /// `embedding_sync::status` distinguishes "stale" from "never synced",
    /// and starts the incremental watcher either way.
    pub stale: bool,
}

/// Resolves the open project and attaches its index store — the prefix every
/// index consumer runs before it can read anything. `Ok(None)` means no
/// project is open, which is a normal state, not an error.
///
/// Deliberately does *not* acquire `EmbeddingSyncGuard`: the two callers
/// disagree about what to do when a sync is in flight, and only they can
/// decide. `embedding_sync::status` waits it out (`lock_sync_guard`);
/// `ai_tools`' semantic search gives up for this one call (`try_lock`, and
/// only on `WouldBlock` — never on `Poisoned`).
pub fn attach_current(
    chunk_index: &ChunkIndex,
    index_store: &IndexStoreSlot,
) -> Result<Option<AttachedIndex>, String> {
    let Some(project) = project_open::get_project().map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let (index_root, storage_dir) = resolve_index_paths(&project)?;
    let (store, stale) = attach_index_store(chunk_index, index_store, &storage_dir, &index_root)?;
    Ok(Some(AttachedIndex { index_root, store, stale }))
}

/// How many vectors are actually resident for `index_root` at the configured
/// provider's dimension count. Read-only: attaches with
/// `allow_repair: false`, so a dimension mismatch reports as an empty index
/// rather than dropping what's persisted for some other provider.
///
/// Only meaningful for a non-stale `AttachedIndex` — a stale store's
/// persisted vectors describe an incompatible chunking, so callers check
/// `AttachedIndex::stale` first rather than trusting a count derived from it.
pub fn embedded_count(
    embedding_index: &EmbeddingIndexSlot,
    store: &IndexStore,
    index_root: &Path,
) -> Result<usize, String> {
    let config = embedding_config::resolve_embedding_config().map_err(|e| e.to_string())?;
    let dimensions = embedding_providers::expected_dimensions(&config);
    attach_embedding_index(embedding_index, store, index_root, dimensions, false)?;
    let slot = embedding_index
        .lock()
        .map_err(|_| "embedding index lock poisoned".to_string())?;
    let (_, _, index) = slot.as_ref().expect("attach_embedding_index just set this");
    Ok(index.len())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::domain::embeddings::{Embedding, EmbeddingError, EmbeddingProviderKind};
    use crate::infra::embedding_credentials_store;
    use crate::services::embedding_config;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    pub(crate) fn fixture_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("alfa-atlas-embeddings-cmd-{label}-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Deterministic fake — never touches `fastembed`/network. Dimension is
    /// configurable so a test can match whatever `expected_dimensions`
    /// resolves to for the real config on the machine running the test.
    pub(crate) struct MockProvider {
        pub(crate) dimensions: usize,
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
    /// whatever `embedding_config::resolve_embedding_config`/
    /// `embedding_credentials_store::get_api_key` actually return on this
    /// machine — `ensure_provider`'s cache check (same config, same key) then
    /// finds a hit and never calls the real `provider_for` (which would load
    /// the ~570MB local ONNX model, or fail outright, if this test's config
    /// happens to be `Local` or an incomplete `Remote` config).
    pub(crate) fn mock_provider_slot() -> EmbeddingProviderSlot {
        let config = embedding_config::resolve_embedding_config().unwrap_or_default();
        let api_key = embedding_credentials_store::get_api_key();
        let dimensions = embedding_providers::expected_dimensions(&config);
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(MockProvider { dimensions });
        EmbeddingProviderSlot::new(Some((config, api_key, provider)))
    }

    /// A temp repo containing `files`, opened as *the* current project, plus
    /// an `EmbeddingSession` wired to fresh slots and the mock provider.
    ///
    /// Runs inside `with_temp_home` because `project_open::get_project` and
    /// `resolve_index_paths` both resolve through the process-global `$HOME`
    /// — without it these tests would fight each other and the developer's
    /// real project state. Returns the *canonicalized* root, since
    /// `open_project` canonicalizes (on macOS `/var/...` -> `/private/var/...`)
    /// and every `FileId` the pipeline produces is relative to that.
    pub(crate) fn with_open_project<T>(
        label: &str,
        files: &[(&str, &str)],
        f: impl FnOnce(&Path, &EmbeddingSession) -> T,
    ) -> T {
        use crate::infra::parsers::registry::ParserRegistry;
        use crate::infra::settings_store::test_support::with_temp_home;

        with_temp_home(|| {
            let root = fixture_dir(label);
            for (name, body) in files {
                fs::write(root.join(name), body).unwrap();
            }
            let root_str = root.to_string_lossy().into_owned();
            project_open::open_project(&root_str, &root_str).unwrap();
            let root = root.canonicalize().unwrap();

            let session = EmbeddingSession {
                repo_index: Arc::new(RepositoryIndex::new()),
                chunk_index: Arc::new(ChunkIndex::new()),
                embedding_index: Arc::new(EmbeddingIndexSlot::new(None)),
                index_store: Arc::new(IndexStoreSlot::new(None)),
                embedding_provider: Arc::new(mock_provider_slot()),
                sync_guard: Arc::new(EmbeddingSyncGuard::new(())),
                index_watcher: Arc::new(IndexWatcherSlot::new(None)),
                workspace_index: Arc::new(WorkspaceIndex::new(ParserRegistry::new())),
                priority_files: Arc::new(PriorityFilesSlot::new(HashSet::new())),
                background_backlog: Arc::new(BackgroundBacklogSlot::new(None)),
                full_sync_active: Arc::new(FullSyncActiveSlot::new(false)),
            };

            let out = f(&root, &session);
            fs::remove_dir_all(&root).ok();
            out
        })
    }


    fn remote_config(model: &str) -> ResolvedEmbeddingConfig {
        ResolvedEmbeddingConfig {
            kind: EmbeddingProviderKind::Remote,
            remote_base_url: Some("https://api.example.com".to_string()),
            remote_model: Some(model.to_string()),
            remote_dimensions: Some(768),
            remote_trusted_cert_pem: None,
            remote_system_id: None,
            remote_disable_tls_verification: false,
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


    #[test]
    fn attach_index_store_clears_chunk_index_on_stale_new_attach() {
        use crate::domain::chunk_index::{ChunkId, ChunkKind, ChunkMetadata};
        use crate::domain::repo_index::Language;

        // Simulate leftover metadata from a previously attached project —
        // the bug that produced FOREIGN KEY failures on resync after a
        // mid-sync project switch onto a never-synced (stale) store.
        let chunk_index = ChunkIndex::new();
        chunk_index.load_metadata(vec![ChunkMetadata {
            id: ChunkId("old.json#0-1".to_string()),
            file_id: FileId("old.json".to_string()),
            language: Language::Json,
            kind: ChunkKind::File,
            start_byte: 0,
            end_byte: 1,
            file_hash: blake3::hash(b"x"),
            hash: blake3::hash(b"y"),
            qualified_name: None,
            ordinal: 0,
        }]);
        assert!(!chunk_index.chunk_ids().is_empty());

        let store_dir = fixture_dir("stale-attach-store");
        let index_root = fixture_dir("stale-attach-root");
        let slot = IndexStoreSlot::new(None);

        // Brand-new store has no version meta → `open_for` reports stale.
        let (_store, stale) =
            attach_index_store(&chunk_index, &slot, &store_dir, &index_root).unwrap();
        assert!(stale);
        assert!(
            chunk_index.chunk_ids().is_empty(),
            "stale attach must clear previous project's chunks, not leave them resident"
        );

        fs::remove_dir_all(&store_dir).ok();
        fs::remove_dir_all(&index_root).ok();
    }

    #[test]
    fn is_current_index_root_is_false_when_no_project_is_open() {
        use crate::infra::settings_store::test_support::with_temp_home;

        with_temp_home(|| {
            assert!(!is_current_index_root(Path::new("/any/repo")));
        });
    }
}
