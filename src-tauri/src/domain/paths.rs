use std::path::{Component, Path, PathBuf};

use super::project_config::ProjectError;

/// Canonicalize `path` and ensure it is equal to or under `root`.
pub fn ensure_under(root: &Path, path: &Path) -> Result<PathBuf, ProjectError> {
    let root = root
        .canonicalize()
        .map_err(ProjectError::Canonicalize)?;
    let canonical = if path.exists() {
        path.canonicalize().map_err(ProjectError::Canonicalize)?
    } else {
        // For not-yet-existing write targets: canonicalize parent + join name.
        let parent = path
            .parent()
            .ok_or_else(|| ProjectError::Message(format!("invalid path: {}", path.display())))?;
        let name = path.file_name().ok_or_else(|| {
            ProjectError::Message(format!("invalid path: {}", path.display()))
        })?;
        let parent = parent
            .canonicalize()
            .map_err(ProjectError::Canonicalize)?;
        parent.join(name)
    };

    if !canonical.starts_with(&root) {
        return Err(ProjectError::PathEscape(canonical.display().to_string()));
    }
    Ok(canonical)
}

/// Relativize `absolute` against `root`. Returns `"."` when equal.
pub fn relative_to(root: &Path, absolute: &Path) -> Result<String, ProjectError> {
    let root = root
        .canonicalize()
        .map_err(ProjectError::Canonicalize)?;
    let absolute = absolute
        .canonicalize()
        .map_err(ProjectError::Canonicalize)?;

    if absolute == root {
        return Ok(".".to_string());
    }

    let rel = absolute
        .strip_prefix(&root)
        .map_err(|_| ProjectError::DocsOutsideRepo(absolute.display().to_string()))?;

    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => {
                return Err(ProjectError::DocsOutsideRepo(
                    absolute.display().to_string(),
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

/// Like `relative_to`, but tolerates `absolute` not existing (canonicalizes
/// its parent + rejoins the file name instead) — mirrors
/// `domain::workspace_index::relative_key_lenient`. Used by the incremental
/// index watcher, which must resolve a `FileId` for `Remove` events (and
/// for `Upserted` events that raced a deletion) where the path is already
/// gone.
pub fn relative_to_lenient(root: &Path, absolute: &Path) -> Result<String, ProjectError> {
    if absolute.exists() {
        return relative_to(root, absolute);
    }
    let root = root.canonicalize().map_err(ProjectError::Canonicalize)?;
    let parent = absolute
        .parent()
        .ok_or_else(|| ProjectError::Message(format!("invalid path: {}", absolute.display())))?;
    let name = absolute
        .file_name()
        .ok_or_else(|| ProjectError::Message(format!("invalid path: {}", absolute.display())))?;
    let parent = parent.canonicalize().map_err(ProjectError::Canonicalize)?;
    let absolute = parent.join(name);

    if absolute == root {
        return Ok(".".to_string());
    }
    let rel = absolute
        .strip_prefix(&root)
        .map_err(|_| ProjectError::DocsOutsideRepo(absolute.display().to_string()))?;

    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => {
                return Err(ProjectError::DocsOutsideRepo(
                    absolute.display().to_string(),
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

/// Join `root` with a relative path that uses `/` separators. Rejects `..`.
pub fn join_relative(root: &Path, relative: &str) -> Result<PathBuf, ProjectError> {
    if relative.is_empty() || relative == "." {
        return Ok(root.to_path_buf());
    }

    let mut out = root.to_path_buf();
    for part in relative.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(ProjectError::PathEscape(relative.to_string()));
        }
        out.push(part);
    }
    Ok(out)
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
        let dir = std::env::temp_dir().join(format!("alfa-atlas-paths-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn relative_and_join_round_trip() {
        let root = temp_dir();
        let nested = root.join("src").join("docs");
        fs::create_dir_all(&nested).unwrap();

        let rel = relative_to(&root, &nested).unwrap();
        assert_eq!(rel, "src/docs");
        let joined = join_relative(&root, &rel).unwrap();
        assert_eq!(joined.canonicalize().unwrap(), nested.canonicalize().unwrap());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_parent_escape() {
        let root = temp_dir();
        assert!(join_relative(&root, "../outside").is_err());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn relative_to_lenient_delegates_when_the_path_still_exists() {
        let root = temp_dir();
        let file = root.join("a.txt");
        fs::write(&file, "x").unwrap();

        assert_eq!(relative_to_lenient(&root, &file).unwrap(), "a.txt");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn relative_to_lenient_resolves_a_since_deleted_file() {
        let root = temp_dir();
        let file = root.join("gone.txt");
        fs::write(&file, "x").unwrap();
        fs::remove_file(&file).unwrap();

        assert_eq!(relative_to_lenient(&root, &file).unwrap(), "gone.txt");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn relative_to_lenient_resolves_a_since_deleted_nested_file() {
        let root = temp_dir();
        let nested_dir = root.join("src").join("docs");
        fs::create_dir_all(&nested_dir).unwrap();
        let file = nested_dir.join("gone.md");
        fs::write(&file, "x").unwrap();
        fs::remove_file(&file).unwrap();

        assert_eq!(relative_to_lenient(&root, &file).unwrap(), "src/docs/gone.md");

        fs::remove_dir_all(&root).ok();
    }
}
