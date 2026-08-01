//! Gitignore-aware workspace scanner built on the `ignore` crate.
//!
//! Returns a flat list of supported files with their modification times. This
//! replaces the manual walk in `services/docs_discovery` (which is the
//! discovery-by-density algorithm, not the index scan).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ignore::WalkBuilder;

use crate::domain::supported_files::is_supported_file;
use crate::domain::workspace_index::WorkspaceIndexError;

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub modified: SystemTime,
}

/// Walk `root` honoring `.gitignore` and the standard skip-list, returning
/// supported files sorted by relative path for deterministic indexing order.
pub fn scan(root: &Path) -> Result<Vec<ScannedFile>, WorkspaceIndexError> {
    walk(root, true)
}

/// Same gitignore-aware walk as `scan`, but returns every file regardless of
/// `is_supported_file` — used by the AI-tools file listing in full-repo
/// mode, where source files (not just doc formats) must be visible to the
/// harness, not just the doc-format subset the editor's own index cares
/// about.
pub fn scan_all(root: &Path) -> Result<Vec<ScannedFile>, WorkspaceIndexError> {
    walk(root, false)
}

fn walk(root: &Path, filter_supported: bool) -> Result<Vec<ScannedFile>, WorkspaceIndexError> {
    let canonical_root = root.canonicalize().map_err(WorkspaceIndexError::Io)?;

    let walker = WalkBuilder::new(&canonical_root)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .parents(true)
        .build();

    let mut files = Vec::new();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path().to_path_buf();
        let path_str = path.to_string_lossy().into_owned();
        if filter_supported && !is_supported_file(&path_str) {
            continue;
        }
        // Skip anything that escapes the canonical root (symlinks, etc.).
        if !path.starts_with(&canonical_root) {
            continue;
        }
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        files.push(ScannedFile { path, modified });
    }

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
}
