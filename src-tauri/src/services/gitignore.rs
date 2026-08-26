use std::fs;
use std::path::Path;

use crate::domain::project_config::ProjectError;

const GITIGNORE_FILE: &str = ".gitignore";

/// Legacy whole-directory ignore — cannot be combined with `!` exceptions
/// inside `.atlas/`, so `ensure_atlas_gitignore` migrates it away.
const LEGACY_ATLAS_ENTRY: &str = ".atlas";

/// Ignore everything under `.atlas/` except the shareable agent memory.
const ATLAS_IGNORE_CONTENTS: &str = ".atlas/*";
const ATLAS_MEMORY_DIR: &str = "!.atlas/memory/";
const ATLAS_MEMORY_TREE: &str = "!.atlas/memory/**";

/// Ensures `entry` is present as its own line in `{repo_root}/.gitignore`,
/// creating the file if it doesn't exist. No-op if already present.
pub fn ensure_entry(repo_root: &str, entry: &str) -> Result<(), ProjectError> {
    let path = Path::new(repo_root).join(GITIGNORE_FILE);

    let existing = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(ProjectError::Read(e)),
    };

    if existing.lines().any(|line| line.trim() == entry) {
        return Ok(());
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(entry);
    updated.push('\n');

    fs::write(&path, updated).map_err(ProjectError::Write)
}

/// Idempotently ensure `.atlas/*` is ignored while `.atlas/memory/` stays
/// trackable. Migrates a legacy bare `.atlas` line (which would block
/// negation rules) into the three-line block.
pub fn ensure_atlas_gitignore(repo_root: &str) -> Result<(), ProjectError> {
    let path = Path::new(repo_root).join(GITIGNORE_FILE);

    let existing = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(ProjectError::Read(e)),
    };

    let mut lines: Vec<String> = existing.lines().map(|l| l.to_string()).collect();
    // Drop the legacy whole-dir ignore so `!.atlas/memory/**` can take effect.
    lines.retain(|line| line.trim() != LEGACY_ATLAS_ENTRY);

    let mut changed = lines.len() != existing.lines().count()
        || (!existing.is_empty() && !existing.ends_with('\n') && lines.is_empty());

    for required in [ATLAS_IGNORE_CONTENTS, ATLAS_MEMORY_DIR, ATLAS_MEMORY_TREE] {
        if !lines.iter().any(|line| line.trim() == required) {
            lines.push(required.to_string());
            changed = true;
        }
    }

    // Also detect the case where we only needed a trailing newline / rewrite
    // after removing `.atlas` but all three required lines were already there.
    let had_legacy = existing.lines().any(|line| line.trim() == LEGACY_ATLAS_ENTRY);
    if !changed && !had_legacy {
        return Ok(());
    }

    let mut updated = lines.join("\n");
    if !updated.is_empty() {
        updated.push('\n');
    }
    fs::write(&path, updated).map_err(ProjectError::Write)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Seven tests in this module call this concurrently. A nanosecond
    /// timestamp alone does not reliably disambiguate them on a coarser
    /// system clock — two would land in the same directory and clobber each
    /// other's `.gitignore`. The counter guarantees uniqueness within the
    /// process regardless of clock resolution, same as
    /// `services::embedding_state::tests::fixture_dir`.
    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("gitignore-test-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn creates_gitignore_when_missing() {
        let dir = temp_dir();
        let root = dir.to_string_lossy().into_owned();

        ensure_entry(&root, ".atlas").unwrap();

        let content = fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(content, ".atlas\n");
    }

    #[test]
    fn appends_to_existing_gitignore() {
        let dir = temp_dir();
        let root = dir.to_string_lossy().into_owned();
        fs::write(dir.join(".gitignore"), "node_modules\n").unwrap();

        ensure_entry(&root, ".atlas").unwrap();

        let content = fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(content, "node_modules\n.atlas\n");
    }

    #[test]
    fn appends_missing_trailing_newline_before_new_entry() {
        let dir = temp_dir();
        let root = dir.to_string_lossy().into_owned();
        fs::write(dir.join(".gitignore"), "node_modules").unwrap();

        ensure_entry(&root, ".atlas").unwrap();

        let content = fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(content, "node_modules\n.atlas\n");
    }

    #[test]
    fn is_idempotent_when_entry_already_present() {
        let dir = temp_dir();
        let root = dir.to_string_lossy().into_owned();
        fs::write(dir.join(".gitignore"), "node_modules\n.atlas\n").unwrap();

        ensure_entry(&root, ".atlas").unwrap();

        let content = fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(content, "node_modules\n.atlas\n");
    }

    #[test]
    fn ensure_atlas_gitignore_writes_exception_block() {
        let dir = temp_dir();
        let root = dir.to_string_lossy().into_owned();

        ensure_atlas_gitignore(&root).unwrap();
        let content = fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(content.contains(".atlas/*\n"));
        assert!(content.contains("!.atlas/memory/\n"));
        assert!(content.contains("!.atlas/memory/**\n"));
        assert!(!content.lines().any(|l| l.trim() == ".atlas"));
    }

    #[test]
    fn ensure_atlas_gitignore_migrates_legacy_atlas_line() {
        let dir = temp_dir();
        let root = dir.to_string_lossy().into_owned();
        fs::write(dir.join(".gitignore"), "node_modules\n.atlas\n").unwrap();

        ensure_atlas_gitignore(&root).unwrap();
        let content = fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(!content.lines().any(|l| l.trim() == ".atlas"));
        assert!(content.contains("node_modules\n"));
        assert!(content.contains(".atlas/*\n"));
        assert!(content.contains("!.atlas/memory/\n"));
        assert!(content.contains("!.atlas/memory/**\n"));
    }

    #[test]
    fn ensure_atlas_gitignore_is_idempotent() {
        let dir = temp_dir();
        let root = dir.to_string_lossy().into_owned();
        ensure_atlas_gitignore(&root).unwrap();
        let first = fs::read_to_string(dir.join(".gitignore")).unwrap();
        ensure_atlas_gitignore(&root).unwrap();
        let second = fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(first, second);
    }
}
