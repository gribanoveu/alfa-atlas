//! Opens the per-project `infra::index_store::IndexStore` (always under
//! `{project_root}/.atlas/index/{mode}` — see `commands::embeddings::
//! resolve_index_paths`) and decides whether its persisted content is safe
//! to reload as-is, or is stale (an incompatible chunking/indexing
//! algorithm version, or a different `index_root`) and must not be
//! trusted. Deliberately read-only: deciding "is this stale" must never
//! itself mutate anything, so it's cheap and safe to run eagerly (e.g.
//! right when a project opens) without blocking on a wipe+rebuild. Only
//! `repair_stale` — called from a real, already mutating `embedding_sync`
//! — actually fixes a stale store.

use std::path::Path;

use crate::domain::chunk_index::CHUNK_VERSION;
use crate::domain::repo_index::INDEX_VERSION;
use crate::infra::index_store::IndexStore;

const META_CHUNK_VERSION: &str = "chunk_version";
const META_INDEX_VERSION: &str = "index_version";
const META_INDEX_ROOT: &str = "index_root";

pub struct IndexAttachment {
    pub store: IndexStore,
    /// `true` if the persisted store's `chunk_version`/`index_version`/
    /// `index_root` meta doesn't match what's expected — its `chunks`/
    /// `files`/`embeddings` rows and `vectors.usearch` must not be loaded
    /// into memory, but are left untouched on disk until `repair_stale`
    /// runs as part of an actual sync.
    pub stale: bool,
}

/// Opens `{storage_dir}/chunks.db` (conventionally
/// `{project_root}/.atlas/index/{mode}`, see `commands::embeddings::
/// resolve_index_paths` — always under the project root, never under
/// `docs_root`, so `.atlas` stays the one place per-project state lives),
/// reporting (but not repairing) staleness against `index_root` (the root
/// this mode actually walks — `docs_root` or `project_root`): an
/// incompatible chunking/indexing algorithm version, or a different
/// `index_root` than last time (e.g. the project itself moved on disk —
/// `AiAccessMode` switches no longer share a `storage_dir` at all, so they
/// can't collide here). A brand-new store (no meta rows yet) is also
/// `stale` — there's nothing to trust yet either way.
pub fn open_for(storage_dir: &Path, index_root: &Path) -> Result<IndexAttachment, String> {
    let store = IndexStore::open(storage_dir).map_err(|e| e.to_string())?;

    let expected_root = index_root.to_string_lossy().to_string();
    let compatible = store.read_meta(META_CHUNK_VERSION).map_err(|e| e.to_string())?
        == Some(CHUNK_VERSION.to_string())
        && store.read_meta(META_INDEX_VERSION).map_err(|e| e.to_string())?
            == Some(INDEX_VERSION.to_string())
        && store.read_meta(META_INDEX_ROOT).map_err(|e| e.to_string())? == Some(expected_root);

    Ok(IndexAttachment { store, stale: !compatible })
}

/// Wipes a stale store's content and rewrites its version/root meta to
/// match the current binary, so the incremental diff that follows treats
/// every current file as new (no leftover incompatible rows to collide
/// with). Only ever called from `commands::embeddings::embedding_sync`,
/// which is about to do a full rebuild anyway — never from a read-only
/// status/attach path.
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
        let attachment = open_for(&root, &root).unwrap();
        assert!(attachment.stale);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn open_for_never_mutates_a_stale_store() {
        let root = fixture_root();
        // First open (brand new -> stale) writes nothing; confirm a second
        // open sees the exact same (still-empty, still-stale) state rather
        // than something `open_for` itself wrote.
        let first = open_for(&root, &root).unwrap();
        assert!(first.stale);
        assert!(first.store.load_all_chunks().unwrap().is_empty());
        assert_eq!(first.store.read_meta("chunk_version").unwrap(), None);
        drop(first);

        let second = open_for(&root, &root).unwrap();
        assert!(second.stale, "still stale — open_for must not have repaired it");
        assert_eq!(second.store.read_meta("chunk_version").unwrap(), None);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn open_for_reports_compatible_after_a_matching_repair() {
        let root = fixture_root();
        let attachment = open_for(&root, &root).unwrap();
        assert!(attachment.stale);
        repair_stale(&attachment.store, &root).unwrap();

        let reopened = open_for(&root, &root).unwrap();
        assert!(!reopened.stale);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn repair_stale_wipes_existing_rows_and_writes_fresh_meta() {
        use crate::domain::chunk_index::{chunk_hash, ChunkId, ChunkKind, ChunkMetadata};
        use crate::domain::repo_index::{FileId, FileMetadata, Language};

        let root = fixture_root();
        let attachment = open_for(&root, &root).unwrap();
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
}
