//! Gitignore-aware workspace scanner built on the `ignore` crate.
//!
//! Returns a flat list of supported files with their modification times. This
//! replaces the manual walk in `services/docs_discovery` (which is the
//! discovery-by-density algorithm, not the index scan).

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::SystemTime;

use ignore::WalkBuilder;

use crate::domain::supported_files::is_supported_file;
use crate::domain::workspace_index::WorkspaceIndexError;

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub modified: SystemTime,
    /// Byte length, from the same `std::fs::metadata` call `modified` is
    /// read from — free to carry along, and what lets a caller cheaply
    /// pre-filter "definitely unchanged since I last saw it" (mtime+size
    /// match) before ever reading the file's content.
    pub size: u64,
}

/// Walk `root` honoring `.gitignore` and the standard skip-list, returning
/// supported files sorted by relative path for deterministic indexing order.
pub fn scan(root: &Path) -> Result<Vec<ScannedFile>, WorkspaceIndexError> {
    walk(root, true, None)
}

/// Same gitignore-aware walk as `scan`, but returns every file regardless of
/// `is_supported_file` — used by the AI-tools file listing in full-repo
/// mode, where source files (not just doc formats) must be visible to the
/// harness, not just the doc-format subset the editor's own index cares
/// about.
pub fn scan_all(root: &Path) -> Result<Vec<ScannedFile>, WorkspaceIndexError> {
    walk(root, false, None)
}

/// Same as `scan_all`, capped to `max_depth` levels below `root`
/// (`ignore::WalkBuilder`'s own convention: `root` itself is depth 0, its
/// direct children are depth 1). `None` = unlimited, identical to
/// `scan_all`. Used by `services::ai_tools::list_full_repo` when a
/// `listFiles` call supplies a `depth` argument.
pub fn scan_all_with_depth(
    root: &Path,
    max_depth: Option<usize>,
) -> Result<Vec<ScannedFile>, WorkspaceIndexError> {
    walk(root, false, max_depth)
}

/// One directory-or-file entry from `scan_all_entries_with_depth` — unlike
/// `ScannedFile`, this carries no mtime/size (nothing consuming it needs
/// staleness info) and includes directories, which `walk`'s other callers
/// (`scan`/`scan_all`/`scan_all_with_depth`, all indexing-focused) deliberately
/// filter out.
#[derive(Debug, Clone)]
pub struct ScannedEntry {
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Same gitignore-aware, depth-limited walk as `scan_all_with_depth`, but
/// includes directory entries (tagged via `is_dir`) instead of silently
/// dropping them — used by `services::ai_tools::list_full_repo` so a
/// `listFiles` call in Full-repo mode can report real directories the way
/// `list_docs_only` already does via `docs_fs::list_docs_tree_scoped`. A new
/// function rather than a `walk()` parameter: `walk` backs `scan`/`scan_all`/
/// `scan_all_with_depth`, all three used for indexing, where a directory
/// entry would just have to be filtered back out by every caller — simpler
/// and lower-risk to leave indexing's walk untouched and add a second,
/// listing-only walk here. `root` itself is never included as an entry —
/// same as `docs_fs::build_dir_children` never emitting a node for the
/// directory it was asked to list, keeping both modes symmetric.
pub fn scan_all_entries_with_depth(
    root: &Path,
    max_depth: Option<usize>,
) -> Result<Vec<ScannedEntry>, WorkspaceIndexError> {
    let canonical_root = root.canonicalize().map_err(WorkspaceIndexError::Io)?;

    let walker = WalkBuilder::new(&canonical_root)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .parents(true)
        .max_depth(max_depth)
        .build();

    let mut entries = Vec::new();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let Some(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path().to_path_buf();
        if path == canonical_root {
            continue;
        }
        // Skip anything that escapes the canonical root (symlinks, etc.).
        if !path.starts_with(&canonical_root) {
            continue;
        }
        entries.push(ScannedEntry { path, is_dir: file_type.is_dir() });
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// Whether `path` (under `root`), just observed via a filesystem-watch
/// `Create` event, would actually be produced by a real gitignore-aware
/// walk of `root` — checked by walking `root` itself, depth-limited to
/// exactly how deep `path` sits below it, rather than the whole tree.
///
/// This deliberately does **not** just walk `path`'s immediate parent
/// directory: `ignore::WalkBuilder` never treats the root it's given as
/// itself excluded (you asked to walk it, so it walks it) — even with
/// `.parents(true)`, which only pulls in ancestor `.gitignore` *rules* for
/// matching this walk's children, not retroactive exclusion of the walk's
/// own starting point. So a rule like `sub/` in the real repo root's
/// `.gitignore` would never be honored by a walk rooted directly at `sub`.
/// Walking from the real `root` instead makes every ancestor directory of
/// `path` (including one matching a whole-directory exclusion rule) a
/// normal child entry subject to the same filtering as anything else,
/// which is what correctly stops the walk from descending into it.
///
/// Still far cheaper than the full recursive walk a real sync does: the
/// `max_depth` cap means only the levels from `root` down to `path`'s own
/// depth are ever listed, never anything below `path`'s own directory.
///
/// Used by the embeddings file watcher (`commands::embeddings::
/// run_incremental_sync`) to decide whether a brand-new, untracked file is
/// safe to index incrementally — extension relevance is already checked
/// by the watcher's own `is_relevant` filter before this ever runs, so
/// this only needs to answer the hidden/gitignore question a full walk
/// would otherwise be the sole way to answer. Returns `false` (fails
/// closed) on any I/O error, including `path` no longer existing by the
/// time this runs (e.g. a fast delete-after-create) or not actually being
/// under `root` at all.
pub fn is_new_file_indexable(root: &Path, path: &Path) -> bool {
    let Ok(canonical_root) = root.canonicalize() else { return false };
    let Ok(canonical_path) = path.canonicalize() else { return false };
    let Some(depth) = canonical_path
        .strip_prefix(&canonical_root)
        .ok()
        .map(|rel| rel.components().count())
        .filter(|&n| n > 0)
    else {
        return false;
    };

    let walker = WalkBuilder::new(&canonical_root)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .parents(true)
        .max_depth(Some(depth))
        .build();

    walker.filter_map(Result::ok).any(|entry| entry.path() == canonical_path)
}

/// The full-tree indexing walk — unlike `is_new_file_indexable`'s
/// depth-capped point check or `scan_all_entries_with_depth`'s own
/// sequential walk, this one genuinely visits every file under `root` (up
/// to `max_depth`), so it's the walk actually worth parallelizing: `ignore`
/// crate's own multi-threaded `WalkBuilder::build_parallel()` (backed by a
/// `available_parallelism`-sized thread pool it manages internally) instead
/// of the single-threaded iterator `.build()` returns. Filtering semantics
/// are identical to before — only the traversal itself is now concurrent;
/// results are collected off an `mpsc::channel` (order is thread-scheduling
/// dependent) and sorted once at the end, so the returned `Vec` is
/// byte-identical to what the old sequential version produced regardless
/// of how work happened to interleave across threads.
fn walk(
    root: &Path,
    filter_supported: bool,
    max_depth: Option<usize>,
) -> Result<Vec<ScannedFile>, WorkspaceIndexError> {
    let canonical_root = root.canonicalize().map_err(WorkspaceIndexError::Io)?;

    let builder = WalkBuilder::new(&canonical_root)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .parents(true)
        .max_depth(max_depth)
        .build_parallel();

    let (tx, rx) = mpsc::channel::<ScannedFile>();

    builder.run(|| {
        let tx = tx.clone();
        let canonical_root = canonical_root.clone();
        Box::new(move |entry| {
            let Ok(entry) = entry else { return ignore::WalkState::Continue };
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                return ignore::WalkState::Continue;
            }
            let path = entry.path().to_path_buf();
            let path_str = path.to_string_lossy().into_owned();
            if filter_supported && !is_supported_file(&path_str) {
                return ignore::WalkState::Continue;
            }
            // Skip anything that escapes the canonical root (symlinks, etc.).
            if !path.starts_with(&canonical_root) {
                return ignore::WalkState::Continue;
            }
            let Ok(meta) = std::fs::metadata(&path) else { return ignore::WalkState::Continue };
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let size = meta.len();
            let _ = tx.send(ScannedFile { path, modified, size });
            ignore::WalkState::Continue
        })
    });
    drop(tx);

    let mut files: Vec<ScannedFile> = rx.into_iter().collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("alfa-atlas-scan-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scans_supported_files() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "= A\n").unwrap();
        fs::write(root.join("b.md"), "# B\n").unwrap();
        fs::write(root.join("c.rs"), "fn c() {}").unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/d.json"), "{}").unwrap();

        let files = scan(&root).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"a.adoc".to_string()));
        assert!(names.contains(&"b.md".to_string()));
        assert!(names.contains(&"d.json".to_string()));
        assert!(!names.contains(&"c.rs".to_string()));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scan_all_includes_unsupported_extensions() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "= A\n").unwrap();
        fs::write(root.join("c.rs"), "fn c() {}").unwrap();

        let files = scan_all(&root).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"a.adoc".to_string()));
        assert!(names.contains(&"c.rs".to_string()));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scan_all_with_depth_limits_recursion() {
        let root = temp_dir();
        fs::write(root.join("a.txt"), "a").unwrap(); // depth 1
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/b.txt"), "b").unwrap(); // depth 2
        fs::create_dir_all(root.join("sub/deeper")).unwrap();
        fs::write(root.join("sub/deeper/c.txt"), "c").unwrap(); // depth 3

        // `walk()` canonicalizes `root` before scanning, so results must be
        // stripped against the same canonical form (e.g. macOS's
        // `/var` -> `/private/var` symlink would otherwise break `strip_prefix`).
        let canonical_root = root.canonicalize().unwrap();
        let names = |files: Vec<ScannedFile>| -> Vec<String> {
            files
                .iter()
                .map(|f| {
                    f.path
                        .strip_prefix(&canonical_root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        };

        let mut at_1 = names(scan_all_with_depth(&root, Some(1)).unwrap());
        at_1.sort();
        assert_eq!(at_1, vec!["a.txt"]);

        let mut at_2 = names(scan_all_with_depth(&root, Some(2)).unwrap());
        at_2.sort();
        assert_eq!(at_2, vec!["a.txt", "sub/b.txt"]);

        let mut unlimited = names(scan_all_with_depth(&root, None).unwrap());
        unlimited.sort();
        assert_eq!(unlimited, vec!["a.txt", "sub/b.txt", "sub/deeper/c.txt"]);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scan_all_entries_with_depth_includes_directories() {
        let root = temp_dir();
        fs::write(root.join("a.txt"), "a").unwrap();
        fs::create_dir_all(root.join("sub/nested")).unwrap();
        fs::write(root.join("sub/b.txt"), "b").unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();

        let canonical_root = root.canonicalize().unwrap();
        let mut names: Vec<(String, bool)> = scan_all_entries_with_depth(&root, None)
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e.path.strip_prefix(&canonical_root).unwrap().to_string_lossy().into_owned(),
                    e.is_dir,
                )
            })
            .collect();
        names.sort();

        assert_eq!(
            names,
            vec![
                ("a.txt".to_string(), false),
                ("empty".to_string(), true),
                ("sub".to_string(), true),
                ("sub/b.txt".to_string(), false),
                ("sub/nested".to_string(), true),
            ]
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scan_all_entries_with_depth_excludes_root_itself() {
        let root = temp_dir();
        fs::write(root.join("a.txt"), "a").unwrap();

        let canonical_root = root.canonicalize().unwrap();
        let entries = scan_all_entries_with_depth(&root, None).unwrap();
        assert!(!entries.iter().any(|e| e.path == canonical_root));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn skips_hidden_and_ignored() {
        let root = temp_dir();
        fs::write(root.join(".hidden.adoc"), "= H\n").unwrap();
        // .gitignore is honored only inside a git repo; here we just check hidden skip.
        let files = scan(&root).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(!names.contains(&".hidden.adoc".to_string()));
        fs::remove_dir_all(&root).ok();
    }

    /// `.gitignore` content is only honored inside a real git repo (see
    /// `skips_hidden_and_ignored`'s own comment) — `is_new_file_indexable`'s
    /// tests that exercise real gitignore matching need one.
    fn temp_git_repo() -> PathBuf {
        let root = temp_dir();
        git2::Repository::init(&root).unwrap();
        root
    }

    #[test]
    fn is_new_file_indexable_true_for_a_plain_new_file() {
        let root = temp_git_repo();
        let path = root.join("new.adoc");
        fs::write(&path, "= New\n").unwrap();

        assert!(is_new_file_indexable(&root, &path));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn is_new_file_indexable_false_for_a_file_ignored_in_its_own_directory() {
        let root = temp_git_repo();
        fs::write(root.join(".gitignore"), "ignored.adoc\n").unwrap();
        let path = root.join("ignored.adoc");
        fs::write(&path, "= Ignored\n").unwrap();

        assert!(!is_new_file_indexable(&root, &path));

        fs::remove_dir_all(&root).ok();
    }

    /// The exclusion rule lives in the repo-root `.gitignore`, one level
    /// above `sub` — proves `.parents(true)` climbs past the immediate
    /// parent (which has no `.gitignore` of its own) to pick it up, even
    /// though the walk itself starts inside `sub`, not at the repo root.
    #[test]
    fn is_new_file_indexable_false_when_an_ancestor_gitignore_excludes_the_directory() {
        let root = temp_git_repo();
        fs::write(root.join(".gitignore"), "sub/\n").unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        let path = root.join("sub/new.adoc");
        fs::write(&path, "= New\n").unwrap();

        assert!(!is_new_file_indexable(&root, &path));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn is_new_file_indexable_false_for_a_hidden_file() {
        let root = temp_git_repo();
        let path = root.join(".hidden.adoc");
        fs::write(&path, "= Hidden\n").unwrap();

        assert!(!is_new_file_indexable(&root, &path));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn is_new_file_indexable_false_for_a_nonexistent_path() {
        let root = temp_git_repo();
        assert!(!is_new_file_indexable(&root, &root.join("never-written.adoc")));
        fs::remove_dir_all(&root).ok();
    }
}
