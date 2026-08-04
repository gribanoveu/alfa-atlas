//! Orchestrates turning a `ChunkIndex` into vectors: `EmbeddingBuilder`
//! calls the configured `EmbeddingProvider`, `EmbeddingIndex` stores the
//! result. Same "builder builds, index stores" split `ChunkBuilder`/
//! `ChunkIndex` already established.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use dashmap::DashMap;

use crate::domain::chunk_index::ChunkId;
use crate::domain::embeddings::{
    Embedding, EmbeddingError, EmbeddingProvider, EmbeddingRecord, SyncStats,
};
use crate::infra::index_store::IndexStore;
use crate::infra::vector_store::{usearch_key, VectorStore};
use crate::services::chunk_builder::ChunkIndex;
use crate::services::chunk_text::resolve_text;

/// Holds only the provider — no result state of its own. Mirrors
/// `ChunkBuilder`.
pub struct EmbeddingBuilder {
    provider: Arc<dyn EmbeddingProvider>,
}

impl EmbeddingBuilder {
    pub fn new(provider: Arc<dyn EmbeddingProvider>) -> Self {
        Self { provider }
    }
}

/// How many pending chunks `EmbeddingIndex::sync` embeds per
/// `EmbeddingProvider::embed` call — bounds how granular `on_progress`
/// reporting is, independent of the local provider's own internal
/// (smaller) batching in `LocalEmbeddingProvider`.
const EMBED_PROGRESS_BATCH: usize = 32;

/// Stores `ChunkId -> EmbeddingRecord` plus the vectors themselves
/// (`VectorStore`/`usearch`). `key_to_chunk` exists only because
/// `usearch_key` is a one-way hash — turning a search result's raw `u64`
/// key back into a `ChunkId` needs a reverse lookup that `VectorStore`
/// itself deliberately doesn't own (see that module's docs).
pub struct EmbeddingIndex {
    records: DashMap<ChunkId, EmbeddingRecord>,
    key_to_chunk: DashMap<u64, ChunkId>,
    vectors: VectorStore,
}

impl EmbeddingIndex {
    pub fn new(dimensions: usize) -> Result<Self, EmbeddingError> {
        Ok(Self {
            records: DashMap::new(),
            key_to_chunk: DashMap::new(),
            vectors: VectorStore::new(dimensions)?,
        })
    }

    /// Restores a whole `EmbeddingIndex` from disk: `vectors` via
    /// `VectorStore::load` (the `usearch` file itself), `records`/
    /// `key_to_chunk` from `persisted_hashes` (`IndexStore::
    /// load_all_embedding_hashes` — the SQLite-backed `chunk_hash` mirror).
    /// Used on cold start so a project reopened in a later session doesn't
    /// need to re-embed anything that hasn't changed since it was last
    /// synced.
    pub fn load(
        dimensions: usize,
        vectors_path: &std::path::Path,
        persisted_hashes: Vec<(ChunkId, blake3::Hash)>,
    ) -> Result<Self, EmbeddingError> {
        let records = DashMap::new();
        let key_to_chunk = DashMap::new();
        for (id, chunk_hash) in persisted_hashes {
            key_to_chunk.insert(usearch_key(&id), id.clone());
            records.insert(id, EmbeddingRecord { chunk_hash });
        }
        Ok(Self {
            records,
            key_to_chunk,
            vectors: VectorStore::load(dimensions, vectors_path)?,
        })
    }

    /// Reconciles this index against `chunk_index`'s current state in one
    /// pass, implementing all three incremental rules: a chunk with no
    /// existing record is embedded; a chunk whose `chunk_hash` no longer
    /// matches its current `ChunkMetadata::hash` is re-embedded; a record
    /// whose `ChunkId` is no longer present in `chunk_index` is removed
    /// from both `records` and the vector store. Chunk hashes already
    /// encode "did the file change *or* did this span's position shift"
    /// (Chunk Index stage) — this doesn't need its own separate staleness
    /// logic, just that one comparison.
    /// `repo_root` is where `resolve_text` reads a pending chunk's text
    /// from — only chunks that are new or whose `hash` changed ever get
    /// their text read; unchanged chunks (the bulk of a repo on a typical
    /// sync) are skipped without touching disk at all. `store`, if given,
    /// is written through at the same points `records` mutates (so the
    /// SQLite-backed `chunk_hash` mirror never drifts from what's actually
    /// in `vectors`), and the vector index is saved back to its `load`ed
    /// path once at the end if anything changed — a full-file `usearch`
    /// write per `sync()` call, not per chunk. `on_progress`, if given, is
    /// called `(chunks_embedded_so_far, total_pending)` after each embed
    /// batch — a plain callback rather than a Tauri `AppHandle` so this
    /// stays testable without a running app; `commands::embeddings` is
    /// where that gets translated into an actual UI event.
    pub fn sync(
        &self,
        chunk_index: &ChunkIndex,
        builder: &EmbeddingBuilder,
        repo_root: &Path,
        store: Option<&IndexStore>,
        on_progress: Option<&dyn Fn(usize, usize)>,
    ) -> Result<SyncStats, EmbeddingError> {
        let mut stats = SyncStats::default();

        let current_ids = chunk_index.chunk_ids();
        let current_set: HashSet<&ChunkId> = current_ids.iter().collect();

        let mut pending: Vec<(ChunkId, String, blake3::Hash)> = Vec::new();
        for id in &current_ids {
            let Some(metadata) = chunk_index.get(id) else {
                continue;
            };
            let unchanged = self
                .records
                .get(id)
                .is_some_and(|record| record.chunk_hash == metadata.hash);
            if unchanged {
                stats.skipped_unchanged += 1;
                continue;
            }
            match resolve_text(repo_root, &metadata) {
                Ok(text) => pending.push((id.clone(), text, metadata.hash)),
                Err(e) => eprintln!("[embedding-index] skipping {}: {e}", id.0),
            }
        }

        let total_pending = pending.len();
        let mut embedded_so_far = 0usize;
        for batch in pending.chunks(EMBED_PROGRESS_BATCH) {
            let texts: Vec<&str> = batch.iter().map(|(_, text, _)| text.as_str()).collect();
            let vectors = builder.provider.embed(&texts)?;
            if vectors.len() != batch.len() {
                return Err(EmbeddingError::Provider(format!(
                    "provider returned {} vectors for {} inputs",
                    vectors.len(),
                    batch.len()
                )));
            }
            for ((id, _, chunk_hash), vector) in batch.iter().zip(vectors) {
                self.vectors.upsert(id, &vector)?;
                self.key_to_chunk.insert(usearch_key(id), id.clone());
                self.records.insert(id.clone(), EmbeddingRecord { chunk_hash: *chunk_hash });
                if let Some(store) = store {
                    store
                        .upsert_embedding(id, *chunk_hash)
                        .map_err(|e| EmbeddingError::Message(e.to_string()))?;
                }
                stats.embedded += 1;
            }
            embedded_so_far += batch.len();
            if let Some(cb) = on_progress {
                cb(embedded_so_far, total_pending);
            }
        }

        let stale: Vec<ChunkId> = self
            .records
            .iter()
            .map(|entry| entry.key().clone())
            .filter(|id| !current_set.contains(id))
            .collect();
        for id in stale {
            self.vectors.remove(&id)?;
            self.key_to_chunk.remove(&usearch_key(&id));
            self.records.remove(&id);
            if let Some(store) = store {
                store
                    .delete_embedding(&id)
                    .map_err(|e| EmbeddingError::Message(e.to_string()))?;
            }
            stats.removed += 1;
        }

        if stats.embedded > 0 || stats.removed > 0 {
            if let Some(path) = self.vectors.path() {
                self.vectors.save(path)?;
            }
        }

        Ok(stats)
    }

    pub fn get(&self, chunk_id: &ChunkId) -> Option<EmbeddingRecord> {
        self.records.get(chunk_id).map(|entry| entry.value().clone())
    }

    pub fn search(
        &self,
        query: &Embedding,
        top_k: usize,
    ) -> Result<Vec<(ChunkId, f32)>, EmbeddingError> {
        let raw = self.vectors.search(query, top_k)?;
        Ok(raw
            .into_iter()
            .filter_map(|(key, distance)| {
                self.key_to_chunk.get(&key).map(|id| (id.clone(), distance))
            })
            .collect())
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) -> Result<(), EmbeddingError> {
        self.records.clear();
        self.key_to_chunk.clear();
        self.vectors.clear()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunk_index::ChunkBuildOptions;
    use crate::services::chunk_builder::ChunkBuilder;
    use crate::services::repo_index::RepositoryIndex;
    use std::fs;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Deterministic fake — never touches real `fastembed`/network. Each
    /// input's embedding is derived from its own length so distinct texts
    /// produce distinct (but reproducible) vectors, and records how many
    /// times it was called so tests can assert unchanged chunks are
    /// skipped rather than merely "not obviously wrong".
    struct MockProvider {
        calls: AtomicUsize,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl EmbeddingProvider for MockProvider {
        fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError> {
            self.calls.fetch_add(texts.len(), Ordering::Relaxed);
            Ok(texts
                .iter()
                .map(|t| Embedding(vec![t.len() as f32, 0.0, 0.0]))
                .collect())
        }

        fn dimensions(&self) -> usize {
            3
        }
    }

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A real repo dir, since `EmbeddingIndex::sync` now reads chunk text
    /// off disk (via `resolve_text`) rather than from an in-memory `Chunk`
    /// — chunks here must come from an actual `RepositoryIndex`/
    /// `ChunkBuilder` pass over real files, not hand-built fixtures.
    fn fixture_repo() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("alfa-atlas-embedding-index-{nanos}-{n}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// Rebuilds `chunk_index` from whatever's on disk under `root` right
    /// now — mirrors the `repo_index.build` + `ChunkBuilder::build_all` +
    /// `chunk_index.clear()/insert_all` sequence `embedding_sync` runs.
    fn rebuild_chunk_index(root: &std::path::Path, chunk_index: &ChunkIndex) {
        let repo_index = RepositoryIndex::new();
        repo_index.build(root).unwrap();
        let chunks = ChunkBuilder::new().build_all(&repo_index, &ChunkBuildOptions::default());
        chunk_index.clear();
        chunk_index.insert_all(chunks);
    }

    #[test]
    fn sync_embeds_new_chunks() {
        let root = fixture_repo();
        fs::write(root.join("a.json"), "0123456789").unwrap();
        let chunk_index = ChunkIndex::new();
        rebuild_chunk_index(&root, &chunk_index);

        let provider = Arc::new(MockProvider::new());
        let builder = EmbeddingBuilder::new(provider.clone());
        let index = EmbeddingIndex::new(3).unwrap();

        let stats = index.sync(&chunk_index, &builder, &root, None, None).unwrap();
        assert_eq!(stats.embedded, 1);
        assert_eq!(stats.skipped_unchanged, 0);
        assert_eq!(stats.removed, 0);
        assert_eq!(index.len(), 1);
        assert_eq!(provider.calls.load(Ordering::Relaxed), 1);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn sync_skips_unchanged_chunks_on_a_second_pass() {
        let root = fixture_repo();
        fs::write(root.join("a.json"), "0123456789").unwrap();
        let chunk_index = ChunkIndex::new();
        rebuild_chunk_index(&root, &chunk_index);

        let provider = Arc::new(MockProvider::new());
        let builder = EmbeddingBuilder::new(provider.clone());
        let index = EmbeddingIndex::new(3).unwrap();

        index.sync(&chunk_index, &builder, &root, None, None).unwrap();
        rebuild_chunk_index(&root, &chunk_index);
        let stats = index.sync(&chunk_index, &builder, &root, None, None).unwrap();

        assert_eq!(stats.embedded, 0);
        assert_eq!(stats.skipped_unchanged, 1);
        // The provider was only ever called for the first pass.
        assert_eq!(provider.calls.load(Ordering::Relaxed), 1);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn sync_re_embeds_a_chunk_whose_hash_changed() {
        let root = fixture_repo();
        let path = root.join("a.json");
        fs::write(&path, "0123456789").unwrap();
        let chunk_index = ChunkIndex::new();
        rebuild_chunk_index(&root, &chunk_index);

        let provider = Arc::new(MockProvider::new());
        let builder = EmbeddingBuilder::new(provider.clone());
        let index = EmbeddingIndex::new(3).unwrap();
        index.sync(&chunk_index, &builder, &root, None, None).unwrap();

        // Same length -> same ChunkId (offsets unchanged), different
        // content -> different file_hash -> different chunk_hash.
        fs::write(&path, "9876543210").unwrap();
        rebuild_chunk_index(&root, &chunk_index);
        let stats = index.sync(&chunk_index, &builder, &root, None, None).unwrap();

        assert_eq!(stats.embedded, 1);
        assert_eq!(stats.skipped_unchanged, 0);
        assert_eq!(provider.calls.load(Ordering::Relaxed), 2);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn sync_removes_records_for_deleted_chunks() {
        let root = fixture_repo();
        fs::write(root.join("a.json"), "0123456789").unwrap();
        let chunk_index = ChunkIndex::new();
        rebuild_chunk_index(&root, &chunk_index);

        let provider = Arc::new(MockProvider::new());
        let builder = EmbeddingBuilder::new(provider);
        let index = EmbeddingIndex::new(3).unwrap();
        index.sync(&chunk_index, &builder, &root, None, None).unwrap();
        assert_eq!(index.len(), 1);

        chunk_index.clear();
        let stats = index.sync(&chunk_index, &builder, &root, None, None).unwrap();

        assert_eq!(stats.removed, 1);
        assert_eq!(index.len(), 0);
        assert!(index
            .get(&crate::domain::chunk_index::ChunkId("a.json#0-10".to_string()))
            .is_none());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn search_translates_vector_store_keys_back_to_chunk_ids() {
        let root = fixture_repo();
        fs::write(root.join("a.json"), "0123456789").unwrap();
        let chunk_index = ChunkIndex::new();
        rebuild_chunk_index(&root, &chunk_index);

        let provider = Arc::new(MockProvider::new());
        let builder = EmbeddingBuilder::new(provider);
        let index = EmbeddingIndex::new(3).unwrap();
        index.sync(&chunk_index, &builder, &root, None, None).unwrap();

        let results = index.search(&Embedding(vec![10.0, 0.0, 0.0]), 1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0 .0, "a.json#0-10");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn clear_empties_records_and_vectors() {
        let root = fixture_repo();
        fs::write(root.join("a.json"), "0123456789").unwrap();
        let chunk_index = ChunkIndex::new();
        rebuild_chunk_index(&root, &chunk_index);

        let provider = Arc::new(MockProvider::new());
        let builder = EmbeddingBuilder::new(provider);
        let index = EmbeddingIndex::new(3).unwrap();
        index.sync(&chunk_index, &builder, &root, None, None).unwrap();

        index.clear().unwrap();
        assert!(index.is_empty());

        fs::remove_dir_all(&root).ok();
    }
}
