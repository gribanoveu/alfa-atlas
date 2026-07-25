use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::paths;
use crate::domain::project_config::{ProjectError, TreeNode};
use crate::domain::supported_files::is_supported_file;

/// List a filtered tree of supported files under `docs_root`.
/// Empty directories are included so newly created folders appear in the UI.
pub fn list_docs_tree(docs_root: &str) -> Result<Vec<TreeNode>, ProjectError> {
    let root = PathBuf::from(docs_root);
    if !root.is_dir() {
        return Err(ProjectError::NotADirectory(docs_root.to_string()));
    }
    let root = root.canonicalize().map_err(ProjectError::Canonicalize)?;
    build_dir_children(&root, &root)
}

fn build_dir_children(docs_root: &Path, dir: &Path) -> Result<Vec<TreeNode>, ProjectError> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(ProjectError::Read)?
        .filter_map(|e| e.ok())
        .collect();

    entries.sort_by(|a, b| {
        let a_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        b_dir
            .cmp(&a_dir)
            .then_with(|| a.file_name().cmp(&b.file_name()))
    });

    let mut nodes = Vec::new();

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }

        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };

        if file_type.is_dir() {
            let children = build_dir_children(docs_root, &path)?;
            let rel = paths::relative_to(docs_root, &path)?;
            nodes.push(TreeNode {
                name,
                path: rel,
                is_dir: true,
                children: Some(children),
            });
        } else if file_type.is_file() {
            let path_str = path.to_string_lossy();
            if !is_supported_file(&path_str) {
                continue;
            }
            let rel = paths::relative_to(docs_root, &path)?;
            nodes.push(TreeNode {
                name,
                path: rel,
                is_dir: false,
                children: None,
            });
        }
    }

    Ok(nodes)
}

pub fn read_project_file(docs_root: &str, relative_path: &str) -> Result<String, ProjectError> {
    let root = resolve_docs_root(docs_root)?;
    let joined = paths::join_relative(&root, relative_path)?;
    let canonical = paths::ensure_under(&root, &joined)?;
    if !canonical.is_file() {
        return Err(ProjectError::NotFound(relative_path.to_string()));
    }
    if !is_supported_file(&canonical.to_string_lossy()) {
        return Err(ProjectError::UnsupportedFile(relative_path.to_string()));
    }
    fs::read_to_string(&canonical).map_err(ProjectError::Read)
}

pub fn write_project_file(
    docs_root: &str,
    relative_path: &str,
    content: &str,
) -> Result<(), ProjectError> {
    let root = resolve_docs_root(docs_root)?;
    let joined = paths::join_relative(&root, relative_path)?;
    let canonical = paths::ensure_under(&root, &joined)?;
    if !is_supported_file(&canonical.to_string_lossy()) {
        return Err(ProjectError::UnsupportedFile(relative_path.to_string()));
    }
    if let Some(parent) = canonical.parent() {
        fs::create_dir_all(parent).map_err(ProjectError::CreateDir)?;
    }
    fs::write(&canonical, content).map_err(ProjectError::Write)
}

/// Create a new empty supported file. Fails if the path already exists.
pub fn create_project_file(docs_root: &str, relative_path: &str) -> Result<(), ProjectError> {
    validate_relative_name(relative_path)?;
    let root = resolve_docs_root(docs_root)?;
    let joined = paths::join_relative(&root, relative_path)?;
    let parent = joined.parent().ok_or_else(|| {
        ProjectError::InvalidName(relative_path.to_string())
    })?;
    let parent = if parent.exists() {
        parent.canonicalize().map_err(ProjectError::Canonicalize)?
    } else {
        fs::create_dir_all(parent).map_err(ProjectError::CreateDir)?;
        parent.canonicalize().map_err(ProjectError::Canonicalize)?
    };
    if !parent.starts_with(&root) {
        return Err(ProjectError::PathEscape(joined.display().to_string()));
    }

    let name = joined.file_name().ok_or_else(|| {
        ProjectError::InvalidName(relative_path.to_string())
    })?;
    let target = parent.join(name);
    if !is_supported_file(&target.to_string_lossy()) {
        return Err(ProjectError::UnsupportedFile(relative_path.to_string()));
    }
    if target.exists() {
        return Err(ProjectError::AlreadyExists(relative_path.to_string()));
    }
    fs::write(&target, "").map_err(ProjectError::Write)
}

/// Create a directory under docs root. Fails if a file already occupies the path.
pub fn create_project_dir(docs_root: &str, relative_path: &str) -> Result<(), ProjectError> {
    validate_relative_name(relative_path)?;
    let root = resolve_docs_root(docs_root)?;
    let joined = paths::join_relative(&root, relative_path)?;
    if joined.exists() {
        if joined.is_dir() {
            return Err(ProjectError::AlreadyExists(relative_path.to_string()));
        }
        return Err(ProjectError::AlreadyExists(relative_path.to_string()));
    }

    // Ensure final path stays under root after creation.
    fs::create_dir_all(&joined).map_err(ProjectError::CreateDir)?;
    let canonical = joined.canonicalize().map_err(ProjectError::Canonicalize)?;
    if !canonical.starts_with(&root) {
        let _ = fs::remove_dir_all(&canonical);
        return Err(ProjectError::PathEscape(relative_path.to_string()));
    }
    Ok(())
}

fn validate_relative_name(relative_path: &str) -> Result<(), ProjectError> {
    let trimmed = relative_path.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Err(ProjectError::InvalidName(relative_path.to_string()));
    }
    for part in trimmed.split(['/', '\\']) {
        if part.is_empty() || part == "." || part == ".." {
            return Err(ProjectError::InvalidName(relative_path.to_string()));
        }
        if part.starts_with('.') {
            return Err(ProjectError::InvalidName(relative_path.to_string()));
        }
    }
    Ok(())
}

fn resolve_docs_root(docs_root: &str) -> Result<PathBuf, ProjectError> {
    let path = Path::new(docs_root);
    if !path.is_dir() {
        return Err(ProjectError::NotADirectory(docs_root.to_string()));
    }
    path.canonicalize().map_err(ProjectError::Canonicalize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("docflow-fs-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn create_file_and_empty_dir_appear_in_tree() {
        let root = temp_dir();
        create_project_dir(root.to_str().unwrap(), "empty-folder").unwrap();
        create_project_file(root.to_str().unwrap(), "empty-folder/note.adoc").unwrap();

        let tree = list_docs_tree(root.to_str().unwrap()).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "empty-folder");
        assert!(tree[0].is_dir);
        let children = tree[0].children.as_ref().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "note.adoc");

        let err = create_project_file(root.to_str().unwrap(), "empty-folder/note.adoc").unwrap_err();
        assert!(matches!(err, ProjectError::AlreadyExists(_)));

        fs::remove_dir_all(&root).ok();
    }
}
