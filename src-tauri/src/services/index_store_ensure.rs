//! Opens the per-project `infra::index_store::IndexStore` and decides
//! whether its persisted content is safe to reload as-is, or must be
//! wiped and rebuilt from scratch — the guard that keeps a stale on-disk
//! cache from ever being trusted silently.

use std::path::Path;

use crate::domain::chunk_index::CHUNK_VERSION;
use crate::domain::repo_index::INDEX_VERSION;
use crate::infra::index_store::IndexStore;

const META_CHUNK_VERSION: &str = "chunk_version";
const META_INDEX_VERSION: &str = "index_version";
const META_INDEX_ROOT: &str = "index_root";

/// Opens `{index_root}/.atlas/index/chunks.db`, wiping its content first if
/// it was written by an incompatible chunking/indexing algorithm version or
/// for a different `index_root` (e.g. `AiAccessMode` toggled between
/// `DocsOnly`/`FullRepo` since the last sync — `FileId` means something
/// different in each mode, so mixing them would silently corrupt lookups).
/// Either way, the returned store's `meta` table always reflects the
/// current versions/`index_root` by the time this returns.
pub fn open_for(index_root: &Path) -> Result<IndexStore, String> {
    let index_dir = index_root.join(".atlas").join("index");
    let store = IndexStore::open(&index_dir).map_err(|e| e.to_string())?;

    let expected_root = index_root.to_string_lossy().to_string();
    let compatible = store.read_meta(META_CHUNK_VERSION).map_err(|e| e.to_string())?
        == Some(CHUNK_VERSION.to_string())
        && store.read_meta(META_INDEX_VERSION).map_err(|e| e.to_string())?
            == Some(INDEX_VERSION.to_string())
        && store.read_meta(META_INDEX_ROOT).map_err(|e| e.to_string())? == Some(expected_root.clone());

    if !compatible {
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
            .write_meta(META_INDEX_ROOT, &expected_root)
            .map_err(|e| e.to_string())?;
    }

    Ok(store)
}
