//! Resolves a chunk's text on demand from its source file. `ChunkIndex`
//! deliberately stores only `ChunkMetadata` (see that module's docs) — a
//! chunk's text is never duplicated in memory, it's read straight out of
//! the file it was sliced from, the same way `ChunkBuilder::build_file`
//! sliced it the first time.

use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use moka::sync::Cache;
use thiserror::Error;

use crate::domain::chunk_index::ChunkMetadata;

/// Total weighted (byte) size the process-wide chunk-text cache may hold —
/// bounded by memory, not entry count, since chunks range up to
/// `DEFAULT_MAX_CHUNK_BYTES` (16KB) each. 64MiB comfortably holds several
/// thousand chunks' text; not user-facing config, just a tuned constant
/// (mirrors `EMBED_PROGRESS_BATCH`/`BACKGROUND_BATCH_FILES` elsewhere in
/// this codebase).
const CHUNK_TEXT_CACHE_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Keyed by `ChunkMetadata.hash` (not `ChunkId`) — `hash` already encodes
/// `file_hash`/`start_byte`/`end_byte`/`CHUNK_VERSION`, so a content change,
/// a position shift, or a chunking-algorithm version bump all naturally
/// produce a different key. A stale entry under an old `hash` is simply
/// never looked up again once nothing holds that old `ChunkMetadata`
/// anymore — no explicit invalidation logic needed, it just becomes dead
/// weight until the weigher below reclaims the space.
fn build_cache(max_bytes: u64) -> Cache<blake3::Hash, Arc<str>> {
    Cache::builder()
        .max_capacity(max_bytes)
        .weigher(|_key: &blake3::Hash, value: &Arc<str>| -> u32 {
            value.len().try_into().unwrap_or(u32::MAX)
        })
        .build()
}

fn text_cache() -> &'static Cache<blake3::Hash, Arc<str>> {
    static CACHE: OnceLock<Cache<blake3::Hash, Arc<str>>> = OnceLock::new();
    CACHE.get_or_init(|| build_cache(CHUNK_TEXT_CACHE_MAX_BYTES))
}

#[derive(Debug, Error)]
pub enum ChunkTextError {
    #[error("io error: {0}")]
    Io(#[source] std::io::Error),
    /// The file's current content no longer hashes to `metadata.file_hash`
    /// — it changed or was deleted since this chunk was indexed. Callers
    /// decide what to do (skip the hit, prompt a resync); this module never
    /// trusts a byte range against content it wasn't computed from.
    #[error("file changed since indexing: {0}")]
    Stale(String),
    /// Defensive only — by construction `start_byte`/`end_byte` are always
    /// valid UTF-8 boundaries within the file they were sliced from, but a
    /// `Stale` file could in principle have a shorter length or a shifted
    /// boundary before that check above ever triggers.
    #[error("chunk range out of bounds for current file content: {0}")]
    OutOfBounds(String),
}

/// Reads `[metadata.start_byte..metadata.end_byte)` from
/// `repo_root.join(&metadata.file_id.0)`, refusing to trust the byte range
/// unless the file's current content still hashes to `metadata.file_hash`.
/// Transparently cached (see `text_cache`) — a hit skips the disk read and
/// the re-hash entirely, returning a cloned copy of the previously resolved
/// text. Only a successful resolution is ever cached; a `ChunkTextError`
/// (file mid-edit, briefly deleted, out of bounds) is always retried fresh
/// on the next call rather than sticky.
pub fn resolve_text(repo_root: &Path, metadata: &ChunkMetadata) -> Result<String, ChunkTextError> {
    if let Some(cached) = text_cache().get(&metadata.hash) {
        return Ok(cached.to_string());
    }

    let path = repo_root.join(&metadata.file_id.0);
    let content = fs::read_to_string(&path).map_err(ChunkTextError::Io)?;

    if blake3::hash(content.as_bytes()) != metadata.file_hash {
        return Err(ChunkTextError::Stale(metadata.file_id.0.clone()));
    }

    let start = metadata.start_byte as usize;
    let end = metadata.end_byte as usize;
    if end > content.len()
        || start > end
        || !content.is_char_boundary(start)
        || !content.is_char_boundary(end)
    {
        return Err(ChunkTextError::OutOfBounds(metadata.id.0.clone()));
    }

    let text = &content[start..end];
    text_cache().insert(metadata.hash, Arc::from(text));
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunk_index::{chunk_hash, ChunkId, ChunkKind};
    use crate::domain::repo_index::{FileId, Language};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture_repo() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("alfa-atlas-chunk-text-{nanos}-{n}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn metadata_for(file_id: &str, file_hash: blake3::Hash, start: u32, end: u32) -> ChunkMetadata {
        ChunkMetadata {
            id: ChunkId(format!("{file_id}#{start}-{end}")),
            file_id: FileId(file_id.to_string()),
            language: Language::Json,
            kind: ChunkKind::File,
            start_byte: start,
            end_byte: end,
            file_hash,
            hash: chunk_hash(file_hash, start, end),
            qualified_name: None,
            ordinal: 0,
        }
    }

    #[test]
    fn resolves_the_exact_byte_range() {
        let root = fixture_repo();
        let content = "0123456789";
        fs::write(root.join("a.json"), content).unwrap();
        let file_hash = blake3::hash(content.as_bytes());

        let metadata = metadata_for("a.json", file_hash, 2, 5);
        let text = resolve_text(&root, &metadata).unwrap();
        assert_eq!(text, "234");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn stale_when_file_content_changed() {
        let root = fixture_repo();
        fs::write(root.join("a.json"), "0123456789").unwrap();
        let stale_hash = blake3::hash(b"different content entirely");

        let metadata = metadata_for("a.json", stale_hash, 0, 3);
        let err = resolve_text(&root, &metadata).unwrap_err();
        assert!(matches!(err, ChunkTextError::Stale(_)));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn io_error_when_file_missing() {
        let root = fixture_repo();
        let metadata = metadata_for("missing.json", blake3::hash(b""), 0, 0);
        let err = resolve_text(&root, &metadata).unwrap_err();
        assert!(matches!(err, ChunkTextError::Io(_)));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn cache_hit_survives_the_source_file_disappearing() {
        let root = fixture_repo();
        let content = "cache me please";
        fs::write(root.join("cached.json"), content).unwrap();
        let file_hash = blake3::hash(content.as_bytes());
        let metadata = metadata_for("cached.json", file_hash, 0, 8);

        // Populates the cache under `metadata.hash`.
        let first = resolve_text(&root, &metadata).unwrap();
        assert_eq!(first, "cache me");

        // Without a cache this would now fail with `ChunkTextError::Io` —
        // the second call must still succeed, proving it never touched disk.
        fs::remove_file(root.join("cached.json")).unwrap();
        let second = resolve_text(&root, &metadata).unwrap();
        assert_eq!(second, "cache me");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn different_content_is_never_served_from_a_stale_cache_entry() {
        let root = fixture_repo();
        let path = root.join("changed.json");

        fs::write(&path, "old-content").unwrap();
        let old_hash = blake3::hash(b"old-content");
        let old_metadata = metadata_for("changed.json", old_hash, 0, 3);
        assert_eq!(resolve_text(&root, &old_metadata).unwrap(), "old");

        // A real re-index would produce fresh metadata (different
        // `file_hash`, hence a different `hash`) for the new content.
        fs::write(&path, "new-content").unwrap();
        let new_hash = blake3::hash(b"new-content");
        let new_metadata = metadata_for("changed.json", new_hash, 0, 3);
        assert_eq!(resolve_text(&root, &new_metadata).unwrap(), "new");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn weigher_bounds_memory_not_entry_count() {
        // A tiny, independent cache — not the process-wide `text_cache()`
        // — so this can exercise eviction without needing to fill 64MiB.
        let cache = build_cache(100);
        let big = "x".repeat(60);

        cache.insert(blake3::hash(b"one"), Arc::from(big.as_str()));
        cache.insert(blake3::hash(b"two"), Arc::from(big.as_str()));
        cache.insert(blake3::hash(b"three"), Arc::from(big.as_str()));
        cache.run_pending_tasks();

        // Three 60-byte entries (180 bytes) can't all fit in a 100-byte
        // budget — at least one must have been evicted, proving the bound
        // is on total weighted size, not on how many entries were inserted.
        assert!(cache.weighted_size() <= 100);
        assert!(cache.entry_count() < 3);
    }
}
