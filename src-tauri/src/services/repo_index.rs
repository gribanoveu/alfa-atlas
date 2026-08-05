//! Orchestrates the Repository Index build: walk the repo, hash + detect
//! language + index each file, store the result. See `domain::repo_index`
//! for the "why" behind the data shapes (no content stored, ranges on every
//! symbol, etc.) and `infra::language_indexers` for the per-language logic.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use dashmap::DashMap;

use crate::domain::paths;
use crate::domain::repo_index::{
    FileId, FileMetadata, IndexedFile, Language, LanguageIndexer, RepoIndexError, INDEX_VERSION,
};
use crate::infra::language_indexers;
use crate::infra::workspace_scanner;

#[derive(Debug, Clone, Default)]
pub struct RepoIndexStats {
    pub index_version: u32,
    pub files_indexed: usize,
    pub by_language: HashMap<Language, usize>,
}

impl RepoIndexStats {
    fn record(&mut self, language: Language) {
        self.files_indexed += 1;
        *self.by_language.entry(language).or_insert(0) += 1;
    }
}

pub struct RepositoryIndex {
    files: DashMap<FileId, IndexedFile>,
    indexers: HashMap<Language, Arc<dyn LanguageIndexer>>,
    /// Set at the start of `build()`, mirrors
    /// `services::workspace_index::WorkspaceIndex`'s own `repo_root` field —
    /// lets `read()` resolve a `FileId` back to an absolute path without
    /// every caller (e.g. `ChunkBuilder`) needing to know the repo layout
    /// itself.
    repo_root: RwLock<Option<PathBuf>>,
}

impl Default for RepositoryIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl RepositoryIndex {
    pub fn new() -> Self {
        Self {
            files: DashMap::new(),
            indexers: language_indexers::default_indexers(),
            repo_root: RwLock::new(None),
        }
    }

    /// Walks `repo_root` and rebuilds the index from scratch. Two distinct
    /// failure policies (see `domain::repo_index` module docs):
    /// an unreadable file is skipped entirely (nothing to hash or index);
    /// a file that reads fine but is malformed for its language still gets
    /// a full record — `LanguageIndexer::index` is infallible, so a broken
    /// file never disappears from the index, only its `symbols` come back
    /// short.
    pub fn build(&self, repo_root: &Path) -> Result<RepoIndexStats, RepoIndexError> {
        self.clear();
        *self.repo_root.write().unwrap() = Some(repo_root.to_path_buf());

        let scanned = workspace_scanner::scan_all(repo_root)?;
        let mut stats = RepoIndexStats {
            index_version: INDEX_VERSION,
            ..Default::default()
        };

        for file in scanned {
            let path_str = file.path.to_string_lossy();
            let Some(language) = crate::domain::repo_index::detect_language(&path_str) else {
                continue;
            };

            let content = match fs::read_to_string(&file.path) {
                Ok(content) => content,
                Err(e) => {
                    eprintln!(
                        "[repo-index] skipping unreadable file {}: {e}",
                        file.path.display()
                    );
                    continue;
                }
            };

            let relative_path = paths::relative_to(repo_root, &file.path)?;
            let hash = blake3::hash(content.as_bytes());
            let metadata = FileMetadata {
                relative_path: relative_path.clone(),
                size_bytes: content.len() as u64,
                modified_at: file.modified,
                hash,
                language,
            };

            let facts = self
                .indexers
                .get(&language)
                .expect("default_indexers registers every Language variant")
                .index(&content);

            stats.record(language);
            self.files.insert(
                FileId(relative_path),
                IndexedFile {
                    metadata,
                    symbols: facts.symbols,
                },
            );
        }

        Ok(stats)
    }

    pub fn get(&self, id: &FileId) -> Option<IndexedFile> {
        self.files.get(id).map(|entry| entry.value().clone())
    }

    pub fn files_for_language(&self, language: Language) -> Vec<IndexedFile> {
        self.files
            .iter()
            .filter(|entry| entry.value().metadata.language == language)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Every indexed file's id — deliberately cheap (clones only the small
    /// keys, not each file's full symbol list) so a caller that wants to
    /// process every file (e.g. `ChunkBuilder::build_all`) doesn't pay for a
    /// bulk copy of the whole index just to enumerate it.
    pub fn file_ids(&self) -> Vec<FileId> {
        self.files.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Reads `file_id`'s current content from disk, resolved against the
    /// `repo_root` passed to the last `build()` call. `RepositoryIndex`
    /// deliberately never stores file content (see `IndexedFile`'s doc
    /// comment) — this is the one place that knows how to turn a `FileId`
    /// back into bytes, so callers (e.g. `ChunkBuilder`) don't need to know
    /// the repo layout themselves.
    pub fn read(&self, file_id: &FileId) -> Result<String, RepoIndexError> {
        let root = self
            .repo_root
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| RepoIndexError::Message("no repo_root set — call build() first".into()))?;
        let path = root.join(&file_id.0);
        fs::read_to_string(&path).map_err(RepoIndexError::Io)
    }

    /// Re-reads, re-hashes, and re-indexes exactly one already-known file —
    /// the single-file counterpart to `build()`'s full walk, for a file
    /// watcher to call instead of a full rescan. Requires `build()` to have
    /// run at least once in this process (same precondition as `read()`) —
    /// `repo_root` isn't known otherwise.
    ///
    /// An I/O error reading the file (most commonly "not found" — the file
    /// vanished between the fs event firing and this running) is
    /// deliberately *not* swallowed here: the caller decides what a missing
    /// file means (the incremental watcher treats it as a delete), this
    /// method just reports what actually happened, same as `read()` does.
    ///
    /// Unsupported-language files are removed instead of erroring — the
    /// file_id might already be tracked from before (e.g. a `.java` file
    /// renamed to something this index doesn't understand).
    pub fn update_file(&self, file_id: &FileId) -> Result<(), RepoIndexError> {
        let root = self
            .repo_root
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| RepoIndexError::Message("no repo_root set — call build() first".into()))?;
        let path = root.join(&file_id.0);

        let Some(language) = crate::domain::repo_index::detect_language(&file_id.0) else {
            self.files.remove(file_id);
            return Ok(());
        };

        let content = fs::read_to_string(&path).map_err(RepoIndexError::Io)?;
        let hash = blake3::hash(content.as_bytes());
        // A second, distinct fs syscall from the read above — if *this one*
        // fails on an otherwise-successfully-read file (an ultra-narrow
        // TOCTOU window), falling back to "now" is correct: the file is
        // demonstrably still there, so treating it as deleted (per the
        // doc comment above) would be wrong.
        let modified_at = fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or_else(|_| std::time::SystemTime::now());
        let metadata = FileMetadata {
            relative_path: file_id.0.clone(),
            size_bytes: content.len() as u64,
            modified_at,
            hash,
            language,
        };

        let facts = self
            .indexers
            .get(&language)
            .expect("default_indexers registers every Language variant")
            .index(&content);

        self.files.insert(
            file_id.clone(),
            IndexedFile {
                metadata,
                symbols: facts.symbols,
            },
        );
        Ok(())
    }

    /// Drops exactly one file's entry — infallible, mirrors
    /// `ChunkIndex::replace_for_file(id, vec![])`'s "always succeeds" shape
    /// so callers don't need error handling on the delete path. A no-op if
    /// `file_id` wasn't tracked.
    pub fn remove_file(&self, file_id: &FileId) {
        self.files.remove(file_id);
    }

    pub fn clear(&self) {
        self.files.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repo_index::SymbolKind;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Mixed-language fixture repo, mirroring the `fixture_repo()` pattern
    /// in `services/ai_tools.rs` tests.
    fn fixture_repo() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("alfa-atlas-repo-index-{nanos}-{n}"));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();

        fs::write(
            root.join("src/UserService.java"),
            "public class UserService {\n    public String getName() { return null; }\n}\n",
        )
        .unwrap();
        fs::write(root.join("src/response.json"), r#"{"a": 1}"#).unwrap();
        fs::write(root.join("src/config.yaml"), "a: 1\n").unwrap();
        fs::write(root.join("docs/README.md"), "# Intro\n\ntext\n").unwrap();
        fs::write(root.join("docs/guide.adoc"), "= Guide\n\ntext\n").unwrap();
        fs::write(root.join("src/Main.rs"), "fn main() {}\n").unwrap();

        root.canonicalize().unwrap()
    }

    #[test]
    fn build_indexes_every_supported_language_and_skips_others() {
        let root = fixture_repo();
        let index = RepositoryIndex::new();
        let stats = index.build(&root).unwrap();

        assert_eq!(stats.index_version, INDEX_VERSION);
        assert_eq!(stats.files_indexed, 5);
        assert_eq!(stats.by_language.get(&Language::Java), Some(&1));
        assert_eq!(stats.by_language.get(&Language::Json), Some(&1));
        assert_eq!(stats.by_language.get(&Language::Yaml), Some(&1));
        assert_eq!(stats.by_language.get(&Language::Markdown), Some(&1));
        assert_eq!(stats.by_language.get(&Language::AsciiDoc), Some(&1));

        assert!(index
            .get(&FileId("src/Main.rs".to_string()))
            .is_none());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn indexed_file_has_metadata_and_symbols_but_no_content() {
        let root = fixture_repo();
        let index = RepositoryIndex::new();
        index.build(&root).unwrap();

        let file = index
            .get(&FileId("src/UserService.java".to_string()))
            .expect("java file indexed");
        assert_eq!(file.metadata.language, Language::Java);
        assert_eq!(
            file.metadata.hash,
            blake3::hash(
                b"public class UserService {\n    public String getName() { return null; }\n}\n"
            )
        );
        assert!(file.metadata.size_bytes > 0);
        assert!(file
            .symbols
            .iter()
            .any(|s| s.name == "UserService" && s.kind == SymbolKind::Class));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn files_for_language_filters_correctly() {
        let root = fixture_repo();
        let index = RepositoryIndex::new();
        index.build(&root).unwrap();

        let java_files = index.files_for_language(Language::Java);
        assert_eq!(java_files.len(), 1);
        assert_eq!(java_files[0].metadata.language, Language::Java);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn file_ids_lists_every_indexed_file() {
        let root = fixture_repo();
        let index = RepositoryIndex::new();
        index.build(&root).unwrap();

        let ids = index.file_ids();
        assert_eq!(ids.len(), 5);
        assert!(ids.contains(&FileId("src/UserService.java".to_string())));
        assert!(!ids.contains(&FileId("src/Main.rs".to_string())));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn read_returns_current_file_content() {
        let root = fixture_repo();
        let index = RepositoryIndex::new();
        index.build(&root).unwrap();

        let content = index.read(&FileId("src/response.json".to_string())).unwrap();
        assert_eq!(content, r#"{"a": 1}"#);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn read_before_build_fails_clearly() {
        let index = RepositoryIndex::new();
        let err = index
            .read(&FileId("src/response.json".to_string()))
            .unwrap_err();
        assert!(matches!(err, RepoIndexError::Message(_)));
    }

    #[test]
    fn update_file_before_build_fails_clearly() {
        let index = RepositoryIndex::new();
        let err = index
            .update_file(&FileId("src/response.json".to_string()))
            .unwrap_err();
        assert!(matches!(err, RepoIndexError::Message(_)));
    }

    #[test]
    fn update_file_reflects_new_content() {
        let root = fixture_repo();
        let index = RepositoryIndex::new();
        index.build(&root).unwrap();

        let file_id = FileId("src/response.json".to_string());
        fs::write(root.join("src/response.json"), r#"{"a": 2}"#).unwrap();
        index.update_file(&file_id).unwrap();

        let updated = index.get(&file_id).unwrap();
        assert_eq!(updated.metadata.hash, blake3::hash(br#"{"a": 2}"#));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn update_file_on_unsupported_extension_removes_a_previously_tracked_entry() {
        let root = fixture_repo();
        let index = RepositoryIndex::new();
        index.build(&root).unwrap();

        // `build()` skips `src/Main.rs` (unsupported language) — simulate it
        // having been tracked anyway (e.g. a stale entry from before a
        // rename), to exercise `update_file`'s "unsupported language ->
        // remove instead of error" branch directly.
        let file_id = FileId("src/Main.rs".to_string());
        index.files.insert(
            file_id.clone(),
            IndexedFile {
                metadata: FileMetadata {
                    relative_path: file_id.0.clone(),
                    size_bytes: 0,
                    modified_at: SystemTime::now(),
                    hash: blake3::hash(b""),
                    language: Language::Java,
                },
                symbols: Vec::new(),
            },
        );
        assert!(index.get(&file_id).is_some());

        index.update_file(&file_id).unwrap();
        assert!(index.get(&file_id).is_none());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn update_file_missing_file_returns_io_not_found() {
        let root = fixture_repo();
        let index = RepositoryIndex::new();
        index.build(&root).unwrap();

        let file_id = FileId("src/response.json".to_string());
        fs::remove_file(root.join("src/response.json")).unwrap();

        let err = index.update_file(&file_id).unwrap_err();
        assert!(matches!(
            err,
            RepoIndexError::Io(e) if e.kind() == std::io::ErrorKind::NotFound
        ));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn remove_file_drops_the_entry_idempotently() {
        let root = fixture_repo();
        let index = RepositoryIndex::new();
        index.build(&root).unwrap();

        let file_id = FileId("src/response.json".to_string());
        assert!(index.get(&file_id).is_some());

        index.remove_file(&file_id);
        assert!(index.get(&file_id).is_none());

        // Idempotent — calling again on an already-absent id must not panic.
        index.remove_file(&file_id);
        assert!(index.get(&file_id).is_none());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unreadable_file_is_skipped_without_failing_the_whole_build() {
        let root = fixture_repo();
        // A broken symlink: detect_language sees it (`.java` extension) but
        // `fs::read_to_string` will fail on it.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("does-not-exist"), root.join("src/Broken.java"))
                .unwrap();
        }

        let index = RepositoryIndex::new();
        let stats = index.build(&root).unwrap();

        // The good Java file is still indexed; the broken symlink is not,
        // and the build as a whole did not fail.
        assert!(index
            .get(&FileId("src/UserService.java".to_string()))
            .is_some());
        assert!(index.get(&FileId("src/Broken.java".to_string())).is_none());
        assert!(stats.files_indexed >= 5);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn malformed_source_file_still_produces_a_record() {
        let root = fixture_repo();
        fs::write(root.join("src/Broken.java"), "public class Broken {\n    public void f( {\n")
            .unwrap();

        let index = RepositoryIndex::new();
        index.build(&root).unwrap();

        let file = index
            .get(&FileId("src/Broken.java".to_string()))
            .expect("malformed file still gets an IndexedFile record");
        assert_eq!(file.metadata.language, Language::Java);
        assert!(file.metadata.size_bytes > 0);
        // Symbols may be empty or partial for malformed input — the point
        // is the record exists at all, not what's in `symbols`.

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn clear_empties_the_index() {
        let root = fixture_repo();
        let index = RepositoryIndex::new();
        index.build(&root).unwrap();
        assert!(index
            .get(&FileId("src/UserService.java".to_string()))
            .is_some());

        index.clear();
        assert!(index
            .get(&FileId("src/UserService.java".to_string()))
            .is_none());

        fs::remove_dir_all(&root).ok();
    }
}
