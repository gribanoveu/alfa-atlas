//! Orchestrates turning a `ChunkIndex` into vectors: `EmbeddingBuilder`
//! calls the configured `EmbeddingProvider`, `EmbeddingIndex` stores the
//! result. Same "builder builds, index stores" split `ChunkBuilder`/
//! `ChunkIndex` already established.

use std::collections::HashSet;
use std::sync::Arc;

use dashmap::DashMap;

use crate::domain::chunk_index::ChunkId;
use crate::domain::embeddings::{
    Embedding, EmbeddingError, EmbeddingProvider, EmbeddingRecord, SyncStats,
};
use crate::infra::vector_store::{usearch_key, VectorStore};
use crate::services::chunk_builder::ChunkIndex;

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

    /// Reconciles this index against `chunk_index`'s current state in one
    /// pass, implementing all three incremental rules: a chunk with no
    /// existing record is embedded; a chunk whose `chunk_hash` no longer
    /// matches its current `ChunkMetadata::hash` is re-embedded; a record
    /// whose `ChunkId` is no longer present in `chunk_index` is removed
    /// from both `records` and the vector store. Chunk hashes already
    /// encode "did the file change *or* did this span's position shift"
    /// (Chunk Index stage) — this doesn't need its own separate staleness
    /// logic, just that one comparison.
    pub fn sync(
        &self,
        chunk_index: &ChunkIndex,
        builder: &EmbeddingBuilder,
    ) -> Result<SyncStats, EmbeddingError> {
        let mut stats = SyncStats::default();

        let current_ids = chunk_index.chunk_ids();
        let current_set: HashSet<&ChunkId> = current_ids.iter().collect();

        let mut pending: Vec<(ChunkId, String, blake3::Hash)> = Vec::new();
        for id in &current_ids {
            let Some(chunk) = chunk_index.get(id) else {
                continue;
            };
            let unchanged = self
                .records
                .get(id)
                .is_some_and(|record| record.chunk_hash == chunk.metadata.hash);
            if unchanged {
                stats.skipped_unchanged += 1;
            } else {
                pending.push((id.clone(), chunk.text, chunk.metadata.hash));
            }
        }

        if !pending.is_empty() {
            let texts: Vec<&str> = pending.iter().map(|(_, text, _)| text.as_str()).collect();
            let vectors = builder.provider.embed(&texts)?;
            if vectors.len() != pending.len() {
                return Err(EmbeddingError::Provider(format!(
                    "provider returned {} vectors for {} inputs",
                    vectors.len(),
                    pending.len()
                )));
            }
            for ((id, _, chunk_hash), vector) in pending.into_iter().zip(vectors) {
                self.vectors.upsert(&id, &vector)?;
                self.key_to_chunk.insert(usearch_key(&id), id.clone());
                self.records.insert(id, EmbeddingRecord { chunk_hash, vector });
                stats.embedded += 1;
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
            stats.removed += 1;
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
    use crate::domain::chunk_index::{ChunkKind, ChunkMetadata};
    use crate::domain::chunk_index::Chunk;
    use crate::domain::repo_index::Language;
    use crate::domain::repo_index::FileId;
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    fn test_chunk(file: &str, start: u32, end: u32, hash_seed: &str) -> Chunk {
        let file_hash = blake3::hash(hash_seed.as_bytes());
        Chunk {
            metadata: ChunkMetadata {
                id: crate::domain::chunk_index::ChunkId(format!("{file}#{start}-{end}")),
                file_id: FileId(file.to_string()),
                language: Language::Java,
                kind: ChunkKind::Method,
                start_byte: start,
                end_byte: end,
                file_hash,
                hash: crate::domain::chunk_index::chunk_hash(file_hash, start, end),
                qualified_name: None,
                ordinal: 0,
            },
            text: "x".repeat((end - start) as usize),
        }
    }

    #[test]
    fn sync_embeds_new_chunks() {
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(vec![test_chunk("A.java", 0, 10, "v1")]);

        let provider = Arc::new(MockProvider::new());
        let builder = EmbeddingBuilder::new(provider.clone());
        let index = EmbeddingIndex::new(3).unwrap();

        let stats = index.sync(&chunk_index, &builder).unwrap();
        assert_eq!(stats.embedded, 1);
        assert_eq!(stats.skipped_unchanged, 0);
        assert_eq!(stats.removed, 0);
        assert_eq!(index.len(), 1);
        assert_eq!(provider.calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn sync_skips_unchanged_chunks_on_a_second_pass() {
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(vec![test_chunk("A.java", 0, 10, "v1")]);

        let provider = Arc::new(MockProvider::new());
        let builder = EmbeddingBuilder::new(provider.clone());
        let index = EmbeddingIndex::new(3).unwrap();

        index.sync(&chunk_index, &builder).unwrap();
        let stats = index.sync(&chunk_index, &builder).unwrap();

        assert_eq!(stats.embedded, 0);
        assert_eq!(stats.skipped_unchanged, 1);
        // The provider was only ever called for the first pass.
        assert_eq!(provider.calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn sync_re_embeds_a_chunk_whose_hash_changed() {
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(vec![test_chunk("A.java", 0, 10, "v1")]);

        let provider = Arc::new(MockProvider::new());
        let builder = EmbeddingBuilder::new(provider.clone());
        let index = EmbeddingIndex::new(3).unwrap();
        index.sync(&chunk_index, &builder).unwrap();

        // Same ChunkId, different file_hash -> different chunk_hash.
        chunk_index.insert_all(vec![test_chunk("A.java", 0, 10, "v2")]);
        let stats = index.sync(&chunk_index, &builder).unwrap();

        assert_eq!(stats.embedded, 1);
        assert_eq!(stats.skipped_unchanged, 0);
        assert_eq!(provider.calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn sync_removes_records_for_deleted_chunks() {
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(vec![test_chunk("A.java", 0, 10, "v1")]);

        let provider = Arc::new(MockProvider::new());
        let builder = EmbeddingBuilder::new(provider);
        let index = EmbeddingIndex::new(3).unwrap();
        index.sync(&chunk_index, &builder).unwrap();
        assert_eq!(index.len(), 1);

        chunk_index.clear();
        let stats = index.sync(&chunk_index, &builder).unwrap();

        assert_eq!(stats.removed, 1);
        assert_eq!(index.len(), 0);
        assert!(index
            .get(&crate::domain::chunk_index::ChunkId(
                "A.java#0-10".to_string()
            ))
            .is_none());
    }

    #[test]
    fn search_translates_vector_store_keys_back_to_chunk_ids() {
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(vec![test_chunk("A.java", 0, 10, "v1")]);

        let provider = Arc::new(MockProvider::new());
        let builder = EmbeddingBuilder::new(provider);
        let index = EmbeddingIndex::new(3).unwrap();
        index.sync(&chunk_index, &builder).unwrap();

        let results = index.search(&Embedding(vec![10.0, 0.0, 0.0]), 1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0 .0, "A.java#0-10");
    }

    #[test]
    fn clear_empties_records_and_vectors() {
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(vec![test_chunk("A.java", 0, 10, "v1")]);
        let provider = Arc::new(MockProvider::new());
        let builder = EmbeddingBuilder::new(provider);
        let index = EmbeddingIndex::new(3).unwrap();
        index.sync(&chunk_index, &builder).unwrap();

        index.clear().unwrap();
        assert!(index.is_empty());
    }
}
