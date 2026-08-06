//! Orchestrates the Repository Index build: walk the repo, hash + detect
//! language + index each file, store the result. See `domain::repo_index`
//! for the "why" behind the data shapes (no content stored, ranges on every
//! symbol, etc.) and `infra::language_indexers` for the per-language logic.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;

use crate::domain::paths;
use crate::domain::repo_index::{
    FileId, FileMetadata, IndexedFile, Language, LanguageIndexer, RepoIndexError, Symbol,
    INDEX_VERSION,
};
use crate::infra::language_indexers;
use crate::infra::workspace_scanner;

/// Compares two `SystemTime`s at whole-second precision — matching what
/// `infra::index_store::IndexStore` actually persists (`mtime_secs`, an
/// `i64` seconds column). A live `fs::metadata` read carries sub-second
/// precision that a value round-tripped through SQLite never does, so
/// comparing them with plain `==` would fail for practically every
/// persisted-but-unchanged file.
fn mtime_secs_eq(a: SystemTime, b: SystemTime) -> bool {
    let secs = |t: SystemTime| t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    secs(a) == secs(b)
}

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
        self.build_internal(repo_root, None)
    }

    /// Same full walk as `build`, but additionally reuses `persisted`'s
    /// file metadata + symbols for a file this project already knew about
    /// before this call — a cold-start caller populates this from
    /// `infra::index_store::IndexStore::load_all_files`/`load_all_symbols`.
    /// `build` itself already gets an equivalent, session-local version of
    /// this for free (see `build_internal`'s `resident` snapshot); this
    /// additionally covers a cold app restart, when nothing is resident yet.
    pub fn build_reusing_symbols(
        &self,
        repo_root: &Path,
        persisted: &HashMap<FileId, (FileMetadata, Vec<Symbol>)>,
    ) -> Result<RepoIndexStats, RepoIndexError> {
        self.build_internal(repo_root, Some(persisted))
    }

    fn build_internal(
        &self,
        repo_root: &Path,
        persisted: Option<&HashMap<FileId, (FileMetadata, Vec<Symbol>)>>,
    ) -> Result<RepoIndexStats, RepoIndexError> {
        // Snapshot whatever's already resident *before* clearing — a file
        // whose content hasn't changed since this session's own last build
        // can reuse its already-parsed symbols too, not only ones supplied
        // via `persisted` (which only a cold-start caller populates from
        // SQLite). This is what makes a second `build()` call in the same
        // session cheap even with no `persisted` map at all.
        let resident: HashMap<FileId, (FileMetadata, Vec<Symbol>)> = self
            .files
            .iter()
            .map(|entry| {
                (
                    entry.key().clone(),
                    (entry.value().metadata.clone(), entry.value().symbols.clone()),
                )
            })
            .collect();

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

            let relative_path = paths::relative_to(repo_root, &file.path)?;
            let file_id = FileId(relative_path.clone());
            let resident_entry = resident.get(&file_id);
            let persisted_entry = persisted.and_then(|p| p.get(&file_id));

            // Cheap pre-filter: mtime+size identical to what we already knew
            // about this file means "almost certainly unchanged" — skip
            // reading and hashing its content entirely, reusing that prior
            // metadata and symbols wholesale. A mismatch here doesn't mean
            // the file changed, only that this heuristic can't confirm it
            // didn't — the fallback below still re-hashes to find out for
            // sure, since mtime/size alone are too weak to trust outright
            // (a `git checkout` can bump mtime with unchanged content; the
            // reverse — identical mtime+size but different content — is the
            // one failure mode this optimization accepts as negligible).
            // Checked against `resident` and `persisted` independently
            // (rather than picking whichever source has *any* entry first)
            // so a `resident` entry that happens not to match still lets a
            // matching `persisted` one win, same as the hash-based fallback
            // below does.
            let mtime_size_match = |metadata: &FileMetadata| {
                metadata.size_bytes == file.size && mtime_secs_eq(metadata.modified_at, file.modified)
            };
            let prefiltered = resident_entry
                .filter(|(metadata, _)| mtime_size_match(metadata))
                .or_else(|| persisted_entry.filter(|(metadata, _)| mtime_size_match(metadata)));
            if let Some((prior_metadata, prior_symbols)) = prefiltered {
                stats.record(language);
                self.files.insert(
                    file_id,
                    IndexedFile {
                        metadata: prior_metadata.clone(),
                        symbols: prior_symbols.clone(),
                    },
                );
                continue;
            }

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

            let hash = blake3::hash(content.as_bytes());
            let metadata = FileMetadata {
                relative_path,
                size_bytes: content.len() as u64,
                modified_at: file.modified,
                hash,
                language,
            };

            // mtime/size looked different (or there was no prior record),
            // but the actual content hash might still match — e.g. a touch
            // with no real edit. Reuse symbols in that case too, without
            // trusting the weaker mtime/size signal for it. `resident` and
            // `persisted` are checked independently here too, same reasoning
            // as the pre-filter above.
            let symbols = resident_entry
                .filter(|(prior_metadata, _)| prior_metadata.hash == hash)
                .or_else(|| persisted_entry.filter(|(prior_metadata, _)| prior_metadata.hash == hash))
                .map(|(_, symbols)| symbols.clone())
                .unwrap_or_else(|| {
                    self.indexers
                        .get(&language)
                        .expect("default_indexers registers every Language variant")
                        .index(&content)
                        .symbols
                });

            stats.record(language);
            self.files.insert(file_id, IndexedFile { metadata, symbols });
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

    /// Every symbol named exactly `name` (case-insensitive — every
    /// supported language's identifiers are ASCII, so
    /// `eq_ignore_ascii_case` is sufficient), across every indexed file —
    /// the cheapest tier of `SemanticSearch`'s cascade, for "where is X
    /// defined" queries. Returns a `Vec` (not a single result) since the
    /// same name can legitimately appear in more than one file.
    pub fn find_symbol(&self, name: &str) -> Vec<(FileId, Symbol)> {
        self.files
            .iter()
            .flat_map(|entry| {
                let file_id = entry.key().clone();
                entry
                    .value()
                    .symbols
                    .iter()
                    .filter(|s| s.name.eq_ignore_ascii_case(name))
                    .map(|s| (file_id.clone(), s.clone()))
                    .collect::<Vec<_>>()
            })
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
    use crate::domain::repo_index::{LanguageFacts, SymbolKind};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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

    /// Records how many times `index()` was actually invoked — lets a test
    /// prove a file was *reused*, not just that its symbols happen to match
    /// what a fresh parse would also produce.
    struct CountingIndexer {
        calls: Arc<AtomicUsize>,
        symbols: Vec<Symbol>,
    }
    impl LanguageIndexer for CountingIndexer {
        fn index(&self, _content: &str) -> LanguageFacts {
            self.calls.fetch_add(1, Ordering::SeqCst);
            LanguageFacts {
                symbols: self.symbols.clone(),
            }
        }
    }

    fn fixed_symbols() -> Vec<Symbol> {
        vec![Symbol {
            name: "Fixed".to_string(),
            kind: SymbolKind::Class,
            start_line: 0,
            end_line: 1,
            start_byte: 0,
            end_byte: 5,
        }]
    }

    fn index_with_counting_java_indexer(calls: Arc<AtomicUsize>) -> RepositoryIndex {
        let mut indexers = language_indexers::default_indexers();
        indexers.insert(
            Language::Java,
            Arc::new(CountingIndexer {
                calls,
                symbols: fixed_symbols(),
            }),
        );
        RepositoryIndex {
            files: DashMap::new(),
            indexers,
            repo_root: RwLock::new(None),
        }
    }

    #[test]
    fn build_reuses_resident_symbols_for_an_unchanged_file_across_calls() {
        let root = fixture_repo();
        let calls = Arc::new(AtomicUsize::new(0));
        let index = index_with_counting_java_indexer(calls.clone());

        index.build(&root).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Same content, same instance — must reuse the resident symbols
        // rather than parsing again.
        index.build(&root).unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second build must reuse resident symbols, not re-parse"
        );

        let result = index
            .get(&FileId("src/UserService.java".to_string()))
            .unwrap();
        assert_eq!(result.symbols, fixed_symbols());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn build_reusing_symbols_skips_content_read_when_mtime_and_size_match() {
        let root = fixture_repo();
        let path = root.join("src/UserService.java");
        let meta = fs::metadata(&path).unwrap();

        // A deliberately *wrong* hash — if this ends up on the resulting
        // `IndexedFile` unchanged, that proves the file's real content was
        // never read/hashed at all, only the mtime+size pre-filter fired.
        let stale_hash = blake3::hash(b"never actually read");
        let prior_metadata = FileMetadata {
            relative_path: "src/UserService.java".to_string(),
            size_bytes: meta.len(),
            modified_at: meta.modified().unwrap(),
            hash: stale_hash,
            language: Language::Java,
        };
        let mut persisted = HashMap::new();
        persisted.insert(
            FileId("src/UserService.java".to_string()),
            (prior_metadata, fixed_symbols()),
        );

        let calls = Arc::new(AtomicUsize::new(0));
        let index = index_with_counting_java_indexer(calls.clone());
        index.build_reusing_symbols(&root, &persisted).unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0, "must not parse — mtime/size matched");
        let result = index
            .get(&FileId("src/UserService.java".to_string()))
            .unwrap();
        assert_eq!(result.symbols, fixed_symbols());
        assert_eq!(result.metadata.hash, stale_hash);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn build_reusing_symbols_skips_content_read_when_mtime_only_matches_at_second_precision() {
        // Regression test: `infra::index_store::IndexStore` persists mtime
        // as whole seconds (`mtime_secs`), so a `persisted` entry loaded
        // after a cold restart never carries the sub-second precision a
        // live `fs::metadata` read has. Truncate to seconds here the same
        // way the store's round-trip does, and confirm the pre-filter still
        // matches instead of silently missing on every persisted entry.
        let root = fixture_repo();
        let path = root.join("src/UserService.java");
        let meta = fs::metadata(&path).unwrap();
        let truncated_modified = UNIX_EPOCH
            + std::time::Duration::from_secs(
                meta.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            );

        let stale_hash = blake3::hash(b"never actually read");
        let prior_metadata = FileMetadata {
            relative_path: "src/UserService.java".to_string(),
            size_bytes: meta.len(),
            modified_at: truncated_modified,
            hash: stale_hash,
            language: Language::Java,
        };
        let mut persisted = HashMap::new();
        persisted.insert(
            FileId("src/UserService.java".to_string()),
            (prior_metadata, fixed_symbols()),
        );

        let calls = Arc::new(AtomicUsize::new(0));
        let index = index_with_counting_java_indexer(calls.clone());
        index.build_reusing_symbols(&root, &persisted).unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "must not parse — mtime/size matched at second precision"
        );
        let result = index
            .get(&FileId("src/UserService.java".to_string()))
            .unwrap();
        assert_eq!(result.symbols, fixed_symbols());
        assert_eq!(result.metadata.hash, stale_hash);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn build_reusing_symbols_falls_back_to_persisted_hash_when_resident_hash_differs() {
        // Regression test: a `resident` entry for a file shouldn't block
        // falling back to a matching `persisted` entry — both sources must
        // be checked independently, not just whichever one has any entry.
        let root = fixture_repo();
        let content = fs::read_to_string(root.join("src/UserService.java")).unwrap();
        let file_hash = blake3::hash(content.as_bytes());

        let calls = Arc::new(AtomicUsize::new(0));
        let index = index_with_counting_java_indexer(calls.clone());

        // Seed `resident` (via a prior `build()`) with a stale hash for this
        // file by pre-inserting an `IndexedFile` whose hash doesn't match
        // current content — `build_internal`'s `resident` snapshot is taken
        // from `self.files` before it clears and rewalks.
        index.files.insert(
            FileId("src/UserService.java".to_string()),
            IndexedFile {
                metadata: FileMetadata {
                    relative_path: "src/UserService.java".to_string(),
                    size_bytes: 999_999,
                    modified_at: SystemTime::UNIX_EPOCH,
                    hash: blake3::hash(b"stale resident content"),
                    language: Language::Java,
                },
                symbols: vec![],
            },
        );

        let mut persisted = HashMap::new();
        let persisted_metadata = FileMetadata {
            relative_path: "src/UserService.java".to_string(),
            size_bytes: 999_999,
            modified_at: SystemTime::UNIX_EPOCH,
            hash: file_hash,
            language: Language::Java,
        };
        persisted.insert(
            FileId("src/UserService.java".to_string()),
            (persisted_metadata, fixed_symbols()),
        );

        index.build_reusing_symbols(&root, &persisted).unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "must not re-parse — persisted hash matched despite stale resident entry"
        );
        let result = index
            .get(&FileId("src/UserService.java".to_string()))
            .unwrap();
        assert_eq!(result.symbols, fixed_symbols());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn build_reusing_symbols_falls_back_to_content_hash_when_mtime_size_dont_match() {
        let root = fixture_repo();
        let content = fs::read_to_string(root.join("src/UserService.java")).unwrap();
        let file_hash = blake3::hash(content.as_bytes());

        // mtime/size are deliberately wrong (so the cheap pre-filter must
        // miss), but the content hash is real — the fallback re-hash should
        // still find a match and reuse symbols instead of re-parsing.
        let prior_metadata = FileMetadata {
            relative_path: "src/UserService.java".to_string(),
            size_bytes: 999_999,
            modified_at: std::time::SystemTime::UNIX_EPOCH,
            hash: file_hash,
            language: Language::Java,
        };
        let mut persisted = HashMap::new();
        persisted.insert(
            FileId("src/UserService.java".to_string()),
            (prior_metadata, fixed_symbols()),
        );

        let calls = Arc::new(AtomicUsize::new(0));
        let index = index_with_counting_java_indexer(calls.clone());
        index.build_reusing_symbols(&root, &persisted).unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "must not re-parse — hash matched persisted despite mtime/size mismatch"
        );
        let result = index
            .get(&FileId("src/UserService.java".to_string()))
            .unwrap();
        assert_eq!(result.symbols, fixed_symbols());
        // Metadata gets refreshed to the freshly-read size once content was
        // actually read, even though symbols were reused.
        assert_eq!(result.metadata.size_bytes, content.len() as u64);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn build_reusing_symbols_reparses_when_content_hash_differs() {
        let root = fixture_repo();
        let stale_metadata = FileMetadata {
            relative_path: "src/UserService.java".to_string(),
            size_bytes: 0,
            modified_at: std::time::SystemTime::UNIX_EPOCH,
            hash: blake3::hash(b"stale content, not what's on disk"),
            language: Language::Java,
        };
        let mut persisted = HashMap::new();
        persisted.insert(
            FileId("src/UserService.java".to_string()),
            (stale_metadata, vec![]),
        );

        let index = RepositoryIndex::new();
        index.build_reusing_symbols(&root, &persisted).unwrap();

        // Neither mtime/size nor hash matched, so this must have re-parsed
        // rather than reusing the (empty) stale entry.
        let result = index
            .get(&FileId("src/UserService.java".to_string()))
            .unwrap();
        assert!(result
            .symbols
            .iter()
            .any(|s| s.name == "UserService" && s.kind == SymbolKind::Class));

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
