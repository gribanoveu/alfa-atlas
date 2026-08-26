//! Opens the per-project `infra::index_store::IndexStore` (a global,
//! per-repository-identity location since the embeddings cache moved out
//! of the repo — see `services::embedding_state::resolve_index_paths`) and
//! decides whether its persisted content is safe to reload as-is, or is
//! stale (an incompatible chunking/indexing algorithm version) and must
//! not be trusted. Deliberately read-only: deciding "is this stale" must
//! never itself mutate anything, so it's cheap and safe to run eagerly
//! (e.g. right when a project opens) without blocking on a wipe+rebuild.
//! Only `repair_stale` — called from a real, already mutating
//! `embedding_sync` — actually fixes a stale store.

use std::path::Path;

use crate::domain::chunk_index::CHUNK_VERSION;
use crate::domain::repo_index::INDEX_VERSION;
use crate::infra::index_store::IndexStore;
use crate::infra::repository_identity;

const META_CHUNK_VERSION: &str = "chunk_version";
const META_INDEX_VERSION: &str = "index_version";
/// Informational only — not part of the staleness check. `storage_dir` is
/// now keyed 1:1 by repository identity (`repository_identity`), so a
/// store being reattached from a different `index_root` than last time is
/// an expected case (the same repo cloned to a second path, or a second
/// worktree), not a fault — unlike before this cache moved out of the
/// repo, when `index_root` and `storage_dir` were the same directory tree
/// and a mismatch really did mean "wrong project's store".
const META_INDEX_ROOT: &str = "index_root";
/// Informational only, from `repository_identity::resolve`. Lets a stored
/// index be compared against what it was last built from without making
/// the revision part of the store's identity or its staleness check — see
/// `services::embedding_state::resolve_index_paths` for why revision is
/// deliberately excluded from `repository_id` itself.
const META_REPOSITORY_URL: &str = "repository_url";
const META_REVISION: &str = "revision";

pub struct IndexAttachment {
    pub store: IndexStore,
    /// `true` if the persisted store's `chunk_version`/`index_version`
    /// meta doesn't match what's expected — its `chunks`/`files`/
    /// `embeddings` rows and `vectors.usearch` must not be loaded into
    /// memory, but are left untouched on disk until `repair_stale` runs as
    /// part of an actual sync.
    pub stale: bool,
}

/// Opens `{storage_dir}/chunks.db`, reporting (but not repairing)
/// staleness: an incompatible chunking/indexing algorithm version. A
/// brand-new store (no meta rows yet) is also `stale` — there's nothing to
/// trust yet either way.
pub fn open_for(storage_dir: &Path) -> Result<IndexAttachment, String> {
    let store = IndexStore::open(storage_dir).map_err(|e| e.to_string())?;

    let compatible = store.read_meta(META_CHUNK_VERSION).map_err(|e| e.to_string())?
        == Some(CHUNK_VERSION.to_string())
        && store.read_meta(META_INDEX_VERSION).map_err(|e| e.to_string())?
            == Some(INDEX_VERSION.to_string());

    Ok(IndexAttachment { store, stale: !compatible })
}

/// Wipes a stale store's content and rewrites its version/identity meta to
/// match the current binary and repository state, so the incremental diff
/// that follows treats every current file as new (no leftover incompatible
/// rows to collide with). Only ever called from `commands::embeddings::
/// embedding_sync`, which is about to do a full rebuild anyway — never
/// from a read-only status/attach path.
pub fn repair_stale(store: &IndexStore, index_root: &Path) -> Result<(), String> {
    store.wipe().map_err(|e| e.to_string())?;
    let vectors_path = store.vectors_path();
    if vectors_path.exists() {
        std::fs::remove_file(&vectors_path).map_err(|e| e.to_string())?;
    }
    store
        .write_meta(META_CHUNK_VERSION, &CHUNK_VERSION.to_string())
        .map_err(|e| e.to_string())?;
    store
        .write_meta(META_INDEX_VERSION, &INDEX_VERSION.to_string())
        .map_err(|e| e.to_string())?;
    store
        .write_meta(META_INDEX_ROOT, &index_root.to_string_lossy())
        .map_err(|e| e.to_string())?;

    let identity = repository_identity::resolve(index_root);
    if let Some(url) = identity.canonical_url {
        store.write_meta(META_REPOSITORY_URL, &url).map_err(|e| e.to_string())?;
    }
    if let Some(revision) = identity.revision {
        store.write_meta(META_REVISION, &revision).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture_root() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("alfa-atlas-index-store-ensure-{nanos}-{n}"))
    }

    #[test]
    fn a_brand_new_store_is_reported_stale() {
        let root = fixture_root();
        let attachment = open_for(&root).unwrap();
        assert!(attachment.stale);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn open_for_never_mutates_a_stale_store() {
        let root = fixture_root();
        // First open (brand new -> stale) writes nothing; confirm a second
        // open sees the exact same (still-empty, still-stale) state rather
        // than something `open_for` itself wrote.
        let first = open_for(&root).unwrap();
        assert!(first.stale);
        assert!(first.store.load_all_chunks().unwrap().is_empty());
        assert_eq!(first.store.read_meta("chunk_version").unwrap(), None);
        drop(first);

        let second = open_for(&root).unwrap();
        assert!(second.stale, "still stale — open_for must not have repaired it");
        assert_eq!(second.store.read_meta("chunk_version").unwrap(), None);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn open_for_reports_compatible_after_a_matching_repair() {
        let root = fixture_root();
        let attachment = open_for(&root).unwrap();
        assert!(attachment.stale);
        repair_stale(&attachment.store, &root).unwrap();

        let reopened = open_for(&root).unwrap();
        assert!(!reopened.stale);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn repair_stale_wipes_existing_rows_and_writes_fresh_meta() {
        use crate::domain::chunk_index::{chunk_hash, ChunkId, ChunkKind, ChunkMetadata};
        use crate::domain::repo_index::{FileId, FileMetadata, Language};

        let root = fixture_root();
        let attachment = open_for(&root).unwrap();
        let store = attachment.store;

        let file_hash = blake3::hash(b"whatever");
        store
            .upsert_files(&[FileMetadata {
                relative_path: "a.json".to_string(),
                size_bytes: 3,
                modified_at: SystemTime::now(),
                hash: file_hash,
                language: Language::Json,
            }])
            .unwrap();
        store
            .replace_chunks_for_file(
                &FileId("a.json".to_string()),
                &[ChunkMetadata {
                    id: ChunkId("a.json#0-3".to_string()),
                    file_id: FileId("a.json".to_string()),
                    language: Language::Json,
                    kind: ChunkKind::File,
                    start_byte: 0,
                    end_byte: 3,
                    file_hash,
                    hash: chunk_hash(file_hash, 0, 3),
                    qualified_name: None,
                    ordinal: 0,
                }],
            )
            .unwrap();
        assert_eq!(store.load_all_chunks().unwrap().len(), 1);

        repair_stale(&store, &root).unwrap();

        assert!(store.load_all_chunks().unwrap().is_empty());
        assert_eq!(
            store.read_meta("chunk_version").unwrap().as_deref(),
            Some(CHUNK_VERSION.to_string().as_str())
        );
        assert_eq!(
            store.read_meta("index_root").unwrap().as_deref(),
            Some(root.to_string_lossy().to_string().as_str())
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn stored_index_root_meta_no_longer_gates_staleness() {
        // Storage is keyed by repository identity now, not by `index_root`
        // — a store built while `repair_stale` was called with one
        // `index_root` (a project at some local path) must still open as
        // compatible later even though the meta it wrote down for that
        // `index_root` doesn't match "wherever the project is now" (a
        // second clone/worktree, or the project simply moved). Unlike the
        // old colocated-storage design, that's expected, not a fault.
        let storage_dir = fixture_root();
        let original_root = fixture_root();
        let attachment = open_for(&storage_dir).unwrap();
        repair_stale(&attachment.store, &original_root).unwrap();

        assert_eq!(
            attachment.store.read_meta("index_root").unwrap().as_deref(),
            Some(original_root.to_string_lossy().to_string().as_str())
        );

        let reopened = open_for(&storage_dir).unwrap();
        assert!(!reopened.stale, "index_root mismatch must not cause staleness");

        std::fs::remove_dir_all(&storage_dir).ok();
    }
}
