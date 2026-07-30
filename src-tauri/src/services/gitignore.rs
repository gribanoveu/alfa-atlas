use std::fs;
use std::path::Path;

use crate::domain::project_config::ProjectError;

const GITIGNORE_FILE: &str = ".gitignore";

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("gitignore-test-{nanos}"));
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
}
