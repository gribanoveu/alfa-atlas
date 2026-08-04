//! Orchestrates turning an already-built `RepositoryIndex` into a
//! `ChunkIndex`: per file, sort its symbols, ask the registered
//! `ChunkStrategy` for ranges, turn each range into a full `Chunk` (text,
//! hash, `qualified_name`, `ordinal`). See `domain::chunk_index` for the
//! "why" behind the data shapes and gap-ownership rules.

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;

use crate::domain::chunk_index::{
    chunk_hash, finalize_ordinals, qualified_name_for, split_oversized_chunks, Chunk,
    ChunkBuildOptions, ChunkId, ChunkMetadata, ChunkStrategy,
};
use crate::domain::repo_index::{FileId, Language, RepoIndexError};
use crate::infra::chunk_strategies;
use crate::services::repo_index::RepositoryIndex;

/// Builds chunks — holds only the strategy registry, no result state.
/// `build_file`/`build_all` exist as two entry points now (`build_all`
/// simply loops `build_file`) so a future file watcher can call
/// `build_file` for exactly the one changed file without either this type
/// or `ChunkIndex` needing to change.
pub struct ChunkBuilder {
    strategies: HashMap<Language, Arc<dyn ChunkStrategy>>,
}

impl Default for ChunkBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkBuilder {
    pub fn new() -> Self {
        Self {
            strategies: chunk_strategies::default_chunk_strategies(),
        }
    }

    /// Builds every chunk for one file. `RepositoryIndex` owns both lookups
    /// this needs (`get` for metadata+symbols, `read` for content) — this
    /// function never touches a `Path` itself.
    pub fn build_file(
        &self,
        repo_index: &RepositoryIndex,
        file_id: &FileId,
        options: &ChunkBuildOptions,
    ) -> Result<Vec<Chunk>, RepoIndexError> {
        let indexed = repo_index.get(file_id).ok_or_else(|| {
            RepoIndexError::Message(format!("no such indexed file: {}", file_id.0))
        })?;
        let content = repo_index.read(file_id)?;

        // The one place this invariant is enforced — strategies receive an
        // already-sorted slice and never re-derive (or assume) the order
        // themselves.
        let mut symbols = indexed.symbols.clone();
        symbols.sort_by_key(|s| s.start_byte);

        let strategy = self
            .strategies
            .get(&indexed.metadata.language)
            .expect("default_chunk_strategies registers every Language variant");
        let spans = strategy.build_spans(&symbols, content.len());

        let file_hash = indexed.metadata.hash;
        let language = indexed.metadata.language;

        let chunks: Vec<Chunk> = spans
            .into_iter()
            .map(|span| {
                let text = content[span.start_byte as usize..span.end_byte as usize].to_string();
                let hash = chunk_hash(file_hash, span.start_byte, span.end_byte);
                let qualified_name = span
                    .anchor_symbol
                    .as_ref()
                    .and_then(|anchor| qualified_name_for(anchor, &symbols));
                Chunk {
                    metadata: ChunkMetadata {
                        id: ChunkId(format!(
                            "{}#{}-{}",
                            file_id.0, span.start_byte, span.end_byte
                        )),
                        file_id: file_id.clone(),
                        language,
                        kind: span.kind,
                        start_byte: span.start_byte,
                        end_byte: span.end_byte,
                        file_hash,
                        hash,
                        qualified_name,
                        ordinal: 0,
                    },
                    text,
                }
            })
            .collect();

        let chunks = split_oversized_chunks(chunks, options.max_chunk_bytes);
        Ok(finalize_ordinals(chunks))
    }

    /// Builds chunks for every file in `repo_index`. A file that fails to
    /// build (missing/unreadable — `build_file`'s `Err` case) is skipped
    /// with a warning, same resilience policy as `RepositoryIndex::build`;
    /// it never aborts the whole pass.
    pub fn build_all(&self, repo_index: &RepositoryIndex, options: &ChunkBuildOptions) -> Vec<Chunk> {
        let mut all = Vec::new();
        for file_id in repo_index.file_ids() {
            match self.build_file(repo_index, &file_id, options) {
                Ok(chunks) => all.extend(chunks),
                Err(e) => eprintln!("[chunk-builder] skipping {}: {e}", file_id.0),
            }
        }
        all
    }
}

/// Stores chunks — no knowledge of how they were built. `DashMap<ChunkId,
/// Chunk>`, same storage style `RepositoryIndex` uses for `IndexedFile`.
pub struct ChunkIndex {
    chunks: DashMap<ChunkId, Chunk>,
}

impl Default for ChunkIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkIndex {
    pub fn new() -> Self {
        Self {
            chunks: DashMap::new(),
        }
    }

    pub fn insert_all(&self, chunks: Vec<Chunk>) {
        for chunk in chunks {
            self.chunks.insert(chunk.metadata.id.clone(), chunk);
        }
    }

    /// Drops every existing chunk for `file_id`, then inserts `chunks` —
    /// the shape a future file watcher calls for exactly the one file that
    /// changed. `O(n)` over the whole map today (no secondary index by
    /// `file_id`); fine at today's scale, and the API doesn't need to
    /// change if that's ever worth optimizing.
    pub fn replace_for_file(&self, file_id: &FileId, chunks: Vec<Chunk>) {
        self.chunks.retain(|_, c| c.metadata.file_id != *file_id);
        self.insert_all(chunks);
    }

    /// Every chunk belonging to one file — re-embedding, deletion, display,
    /// debug all want this without scanning the whole map by hand each time.
    pub fn chunks_for_file(&self, file_id: &FileId) -> Vec<Chunk> {
        self.chunks
            .iter()
            .filter(|entry| entry.value().metadata.file_id == *file_id)
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn get(&self, id: &ChunkId) -> Option<Chunk> {
        self.chunks.get(id).map(|entry| entry.value().clone())
    }

    /// Every chunk id currently stored, across every file — cheap (clones
    /// only the small keys, not each chunk's text), mirrors
    /// `RepositoryIndex::file_ids()`. What `EmbeddingIndex::sync` diffs
    /// against to detect chunks that no longer exist.
    pub fn chunk_ids(&self) -> Vec<ChunkId> {
        self.chunks.iter().map(|entry| entry.key().clone()).collect()
    }

    pub fn clear(&self) {
        self.chunks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunk_index::ChunkKind;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture_repo() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("alfa-atlas-chunk-builder-{nanos}-{n}"));
        fs::create_dir_all(root.join("src")).unwrap();
        root
    }

    const JAVA_SAMPLE: &str = r#"package com.example;

import com.example.UserRepository;

@Service
@RequiredArgsConstructor
public class UserService {
    private final UserRepository repository;

    @Transactional
    public void save() {
        repository.save();
    }

    @Transactional
    public void delete() {
        repository.delete();
    }
}
"#;

    /// Every chunk's range, sorted by `start_byte`, is contiguous and
    /// gapless, covers the whole file, and doesn't overlap — the invariant
    /// every fixture below is checked against.
    fn assert_full_coverage_no_overlap(chunks: &[Chunk], file_len: usize) {
        let mut sorted: Vec<&Chunk> = chunks.iter().collect();
        sorted.sort_by_key(|c| c.metadata.start_byte);

        assert!(!sorted.is_empty(), "expected at least one chunk");
        assert_eq!(sorted[0].metadata.start_byte, 0, "must start at byte 0");
        for pair in sorted.windows(2) {
            assert_eq!(
                pair[0].metadata.end_byte, pair[1].metadata.start_byte,
                "chunks must be contiguous with no gap or overlap"
            );
        }
        assert_eq!(
            sorted.last().unwrap().metadata.end_byte,
            file_len as u32,
            "must cover through end of file"
        );
        for (i, chunk) in sorted.iter().enumerate() {
            assert_eq!(chunk.metadata.ordinal, i as u32);
        }
    }

    #[test]
    fn java_file_splits_into_field_and_method_chunks_with_qualified_names() {
        let root = fixture_repo();
        fs::write(root.join("src/UserService.java"), JAVA_SAMPLE).unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let builder = ChunkBuilder::new();
        let options = ChunkBuildOptions::default();

        let file_id = FileId("src/UserService.java".to_string());
        let chunks = builder.build_file(&repo_index, &file_id, &options).unwrap();

        assert_eq!(chunks.len(), 3);
        let by_name: HashMap<&str, &Chunk> = chunks
            .iter()
            .map(|c| (c.metadata.qualified_name.as_deref().unwrap(), c))
            .collect();

        let field = by_name["UserService.repository"];
        assert_eq!(field.metadata.kind, ChunkKind::Field);
        assert_eq!(field.metadata.start_byte, 0);
        assert!(field.text.contains("package com.example"));
        assert!(field.text.contains("private final UserRepository repository"));

        let save = by_name["UserService.save"];
        assert_eq!(save.metadata.kind, ChunkKind::Method);
        assert_eq!(save.metadata.start_byte, field.metadata.end_byte);
        assert!(save.text.contains("@Transactional"));
        assert!(save.text.contains("public void save()"));
        assert!(!save.text.contains("repository;"));

        let delete = by_name["UserService.delete"];
        assert_eq!(delete.metadata.kind, ChunkKind::Method);
        assert_eq!(delete.metadata.start_byte, save.metadata.end_byte);
        assert_eq!(delete.metadata.end_byte as usize, JAVA_SAMPLE.len());
        assert!(delete.text.contains("public void delete()"));

        assert_full_coverage_no_overlap(&chunks, JAVA_SAMPLE.len());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn oversized_method_is_split_but_still_covers_the_file() {
        let root = fixture_repo();
        let big_body = "        noop();\n".repeat(2000); // well over 16KB
        let content = format!(
            "public class Big {{\n    public void run() {{\n{big_body}    }}\n}}\n"
        );
        fs::write(root.join("src/Big.java"), &content).unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let builder = ChunkBuilder::new();
        let options = ChunkBuildOptions::default();

        let file_id = FileId("src/Big.java".to_string());
        let chunks = builder.build_file(&repo_index, &file_id, &options).unwrap();

        assert!(chunks.len() > 1, "oversized method should split");
        for chunk in &chunks {
            assert!(chunk.text.len() <= options.max_chunk_bytes);
            assert_eq!(chunk.metadata.qualified_name.as_deref(), Some("Big.run"));
        }
        assert_full_coverage_no_overlap(&chunks, content.len());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn markdown_splits_by_heading() {
        let root = fixture_repo();
        let content = "# Title\n\nintro\n\n## Errors\n\nsome errors\n\n## Notes\n\nfinal notes\n";
        fs::write(root.join("src/README.md"), content).unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let builder = ChunkBuilder::new();
        let options = ChunkBuildOptions::default();

        let file_id = FileId("src/README.md".to_string());
        let chunks = builder.build_file(&repo_index, &file_id, &options).unwrap();

        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|c| c.metadata.kind == ChunkKind::Section));
        assert_full_coverage_no_overlap(&chunks, content.len());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn asciidoc_splits_by_section() {
        let root = fixture_repo();
        let content = "= Guide\n\nintro\n\n== Errors\n\nsome errors\n";
        fs::write(root.join("src/guide.adoc"), content).unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let builder = ChunkBuilder::new();
        let options = ChunkBuildOptions::default();

        let file_id = FileId("src/guide.adoc".to_string());
        let chunks = builder.build_file(&repo_index, &file_id, &options).unwrap();

        assert_eq!(chunks.len(), 2);
        assert_full_coverage_no_overlap(&chunks, content.len());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn json_and_yaml_produce_exactly_one_file_chunk() {
        let root = fixture_repo();
        fs::write(root.join("src/response.json"), r#"{"a": {"b": "c:d"}}"#).unwrap();
        fs::write(root.join("src/config.yaml"), "a: 1\nb: 2\n").unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let builder = ChunkBuilder::new();
        let options = ChunkBuildOptions::default();

        for (path, expected_text) in [
            ("src/response.json", r#"{"a": {"b": "c:d"}}"#),
            ("src/config.yaml", "a: 1\nb: 2\n"),
        ] {
            let file_id = FileId(path.to_string());
            let chunks = builder.build_file(&repo_index, &file_id, &options).unwrap();
            assert_eq!(chunks.len(), 1);
            assert_eq!(chunks[0].metadata.kind, ChunkKind::File);
            assert_eq!(chunks[0].text, expected_text);
            assert_eq!(chunks[0].metadata.qualified_name, None);
        }

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn empty_class_falls_back_to_one_whole_file_chunk() {
        let root = fixture_repo();
        let content = "public class Empty {\n}\n";
        fs::write(root.join("src/Empty.java"), content).unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let builder = ChunkBuilder::new();
        let options = ChunkBuildOptions::default();

        let file_id = FileId("src/Empty.java".to_string());
        let chunks = builder.build_file(&repo_index, &file_id, &options).unwrap();

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].metadata.kind, ChunkKind::File);
        assert_eq!(chunks[0].text, content);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn every_chunk_file_hash_matches_the_indexed_file_hash() {
        let root = fixture_repo();
        fs::write(root.join("src/UserService.java"), JAVA_SAMPLE).unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let file_id = FileId("src/UserService.java".to_string());
        let indexed = repo_index.get(&file_id).unwrap();

        let builder = ChunkBuilder::new();
        let chunks = builder
            .build_file(&repo_index, &file_id, &ChunkBuildOptions::default())
            .unwrap();

        for chunk in &chunks {
            assert_eq!(chunk.metadata.file_hash, indexed.metadata.hash);
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn build_all_covers_every_indexed_file() {
        let root = fixture_repo();
        fs::write(root.join("src/UserService.java"), JAVA_SAMPLE).unwrap();
        fs::write(root.join("src/response.json"), r#"{"a": 1}"#).unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let builder = ChunkBuilder::new();

        let chunks = builder.build_all(&repo_index, &ChunkBuildOptions::default());
        assert_eq!(chunks.len(), 4); // 3 Java + 1 JSON

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn chunk_index_insert_all_and_get_round_trip() {
        let root = fixture_repo();
        fs::write(root.join("src/response.json"), r#"{"a": 1}"#).unwrap();
        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let builder = ChunkBuilder::new();
        let file_id = FileId("src/response.json".to_string());
        let chunks = builder
            .build_file(&repo_index, &file_id, &ChunkBuildOptions::default())
            .unwrap();
        let id = chunks[0].metadata.id.clone();

        let index = ChunkIndex::new();
        index.insert_all(chunks);
        assert!(index.get(&id).is_some());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn chunk_ids_lists_every_stored_chunk_across_files() {
        let root = fixture_repo();
        fs::write(root.join("src/UserService.java"), JAVA_SAMPLE).unwrap();
        fs::write(root.join("src/response.json"), r#"{"a": 1}"#).unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let builder = ChunkBuilder::new();
        let all_chunks = builder.build_all(&repo_index, &ChunkBuildOptions::default());

        let index = ChunkIndex::new();
        index.insert_all(all_chunks.clone());

        let ids = index.chunk_ids();
        assert_eq!(ids.len(), all_chunks.len());
        for chunk in &all_chunks {
            assert!(ids.contains(&chunk.metadata.id));
        }

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn replace_for_file_swaps_only_that_files_chunks() {
        let root = fixture_repo();
        fs::write(root.join("src/UserService.java"), JAVA_SAMPLE).unwrap();
        fs::write(root.join("src/response.json"), r#"{"a": 1}"#).unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&root).unwrap();
        let builder = ChunkBuilder::new();
        let options = ChunkBuildOptions::default();

        let java_id = FileId("src/UserService.java".to_string());
        let json_id = FileId("src/response.json".to_string());

        let index = ChunkIndex::new();
        index.insert_all(builder.build_file(&repo_index, &java_id, &options).unwrap());
        index.insert_all(builder.build_file(&repo_index, &json_id, &options).unwrap());

        assert_eq!(index.chunks_for_file(&java_id).len(), 3);
        assert_eq!(index.chunks_for_file(&json_id).len(), 1);

        // Simulate a rebuild of just the Java file producing a different
        // (here: empty) chunk set.
        index.replace_for_file(&java_id, Vec::new());

        assert_eq!(index.chunks_for_file(&java_id).len(), 0);
        assert_eq!(
            index.chunks_for_file(&json_id).len(),
            1,
            "other files' chunks must be untouched"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn clear_empties_the_chunk_index() {
        let index = ChunkIndex::new();
        index.insert_all(vec![]);
        index.clear();
        assert_eq!(index.chunks_for_file(&FileId("x".to_string())).len(), 0);
    }
}
