//! Wraps `usearch::Index` (embedded HNSW/ANN, no server) — the vector
//! storage half of the embedding layer. Metadata/text stays in
//! `ChunkIndex`; this only ever holds `key -> vector`.
//!
//! `usearch` keys entries by `u64`, not arbitrary strings, so `usearch_key`
//! derives one deterministically from a `ChunkId`: the same `ChunkId`
//! always maps to the same key (so re-embedding a changed chunk upserts
//! rather than duplicates), and collision probability is negligible at any
//! realistic repo size — no separate bidirectional id-mapping table is
//! needed to compute it. Translating a *search result* key back to a
//! `ChunkId`, though, does need a reverse map — `usearch_key` is one-way —
//! which is why `services::embedding_index::EmbeddingIndex` (not this
//! module) keeps `key -> ChunkId` alongside its `ChunkId -> EmbeddingRecord`
//! map.

use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

use crate::domain::chunk_index::ChunkId;
use crate::domain::embeddings::{Embedding, EmbeddingError};

pub fn usearch_key(chunk_id: &ChunkId) -> u64 {
    let hash = blake3::hash(chunk_id.0.as_bytes());
    let bytes: [u8; 8] = hash.as_bytes()[..8]
        .try_into()
        .expect("blake3 hash is at least 8 bytes");
    u64::from_le_bytes(bytes)
}

pub struct VectorStore {
    index: Index,
}

impl VectorStore {
    pub fn new(dimensions: usize) -> Result<Self, EmbeddingError> {
        let options = IndexOptions {
            dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            ..Default::default()
        };
        let index = Index::new(&options).map_err(vector_store_err)?;
        index.reserve(1024).map_err(vector_store_err)?;
        Ok(Self { index })
    }

    /// Adds or replaces the vector for `chunk_id`. `usearch` has no native
    /// upsert — an existing key is removed first so re-embedding a changed
    /// chunk never leaves a stale duplicate entry behind.
    pub fn upsert(&self, chunk_id: &ChunkId, vector: &Embedding) -> Result<(), EmbeddingError> {
        let key = usearch_key(chunk_id);
        if self.index.contains(key) {
            self.index.remove(key).map_err(vector_store_err)?;
        }
        if self.index.size() >= self.index.capacity() {
            let next = (self.index.capacity().max(1)) * 2;
            self.index.reserve(next).map_err(vector_store_err)?;
        }
        self.index.add(key, vector.0.as_slice()).map_err(vector_store_err)?;
        Ok(())
    }

    pub fn remove(&self, chunk_id: &ChunkId) -> Result<(), EmbeddingError> {
        let key = usearch_key(chunk_id);
        if self.index.contains(key) {
            self.index.remove(key).map_err(vector_store_err)?;
        }
        Ok(())
    }

    /// Raw `usearch` keys + cosine distances — callers translate keys back
    /// to `ChunkId`s themselves (see module docs for why this module can't
    /// do that translation itself).
    pub fn search(&self, query: &Embedding, top_k: usize) -> Result<Vec<(u64, f32)>, EmbeddingError> {
        let matches = self
            .index
            .search(query.0.as_slice(), top_k)
            .map_err(vector_store_err)?;
        Ok(matches.keys.into_iter().zip(matches.distances).collect())
    }

    pub fn len(&self) -> usize {
        self.index.size()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) -> Result<(), EmbeddingError> {
        self.index.reset().map_err(vector_store_err)
    }
}

fn vector_store_err(e: cxx::Exception) -> EmbeddingError {
    EmbeddingError::VectorStore(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usearch_key_is_deterministic_and_distinguishes_ids() {
        let a = ChunkId("src/A.java#0-10".to_string());
        let b = ChunkId("src/B.java#0-10".to_string());
        assert_eq!(usearch_key(&a), usearch_key(&a));
        assert_ne!(usearch_key(&a), usearch_key(&b));
    }

    #[test]
    fn upsert_search_and_remove_round_trip() {
        let store = VectorStore::new(3).unwrap();
        let id = ChunkId("f#0-1".to_string());
        store.upsert(&id, &Embedding(vec![1.0, 0.0, 0.0])).unwrap();
        assert_eq!(store.len(), 1);

        let results = store.search(&Embedding(vec![1.0, 0.0, 0.0]), 1).unwrap();
        assert_eq!(results[0].0, usearch_key(&id));

        // Re-upserting the same id replaces rather than duplicates.
        store.upsert(&id, &Embedding(vec![0.0, 1.0, 0.0])).unwrap();
        assert_eq!(store.len(), 1);

        store.remove(&id).unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn clear_empties_the_store() {
        let store = VectorStore::new(2).unwrap();
        store
            .upsert(&ChunkId("f#0-1".to_string()), &Embedding(vec![1.0, 0.0]))
            .unwrap();
        assert_eq!(store.len(), 1);
        store.clear().unwrap();
        assert!(store.is_empty());
    }
}
