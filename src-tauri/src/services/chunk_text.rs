//! Resolves a chunk's text on demand from its source file. `ChunkIndex`
//! deliberately stores only `ChunkMetadata` (see that module's docs) — a
//! chunk's text is never duplicated in memory, it's read straight out of
//! the file it was sliced from, the same way `ChunkBuilder::build_file`
//! sliced it the first time.

use std::fs;
use std::path::Path;

use thiserror::Error;

use crate::domain::chunk_index::ChunkMetadata;

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
pub fn resolve_text(repo_root: &Path, metadata: &ChunkMetadata) -> Result<String, ChunkTextError> {
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

    Ok(content[start..end].to_string())
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
}
