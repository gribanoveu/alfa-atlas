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
        let dir = std::env::temp_dir().join(format!("docflow-paths-{nanos}"));
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
}
