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

/// Delete a file under docs root. Fails if missing or not a file.
pub fn delete_project_file(docs_root: &str, relative_path: &str) -> Result<(), ProjectError> {
    validate_relative_name(relative_path)?;
    let root = resolve_docs_root(docs_root)?;
    let joined = paths::join_relative(&root, relative_path)?;
    let canonical = paths::ensure_under(&root, &joined)?;
    if !canonical.is_file() {
        return Err(ProjectError::NotFound(relative_path.to_string()));
    }
    fs::remove_file(&canonical).map_err(ProjectError::Delete)
}

/// Delete a directory under docs root. Fails if missing, not a directory,
/// or if the target is the docs root itself.
pub fn delete_project_dir(docs_root: &str, relative_path: &str) -> Result<(), ProjectError> {
    validate_relative_name(relative_path)?;
    let root = resolve_docs_root(docs_root)?;
    let joined = paths::join_relative(&root, relative_path)?;
    let canonical = paths::ensure_under(&root, &joined)?;
    if canonical == root {
        return Err(ProjectError::InvalidName(relative_path.to_string()));
    }
    if !canonical.is_dir() {
        return Err(ProjectError::NotFound(relative_path.to_string()));
    }
    fs::remove_dir_all(&canonical).map_err(ProjectError::Delete)
}

/// Rename a file under docs root. Only the basename changes; the parent
/// directory is preserved. Fails if the source is missing, the destination
/// already exists, or the new name is not a supported file type.
pub fn rename_project_file(
    docs_root: &str,
    from_relative: &str,
    to_relative: &str,
) -> Result<(), ProjectError> {
    validate_relative_name(from_relative)?;
    validate_relative_name(to_relative)?;
    let root = resolve_docs_root(docs_root)?;
    let from_joined = paths::join_relative(&root, from_relative)?;
    let from_canonical = paths::ensure_under(&root, &from_joined)?;
    if !from_canonical.is_file() {
        return Err(ProjectError::NotFound(from_relative.to_string()));
    }
    let to_joined = paths::join_relative(&root, to_relative)?;
    let to_canonical = paths::ensure_under(&root, &to_joined)?;
    if !is_supported_file(&to_canonical.to_string_lossy()) {
        return Err(ProjectError::UnsupportedFile(to_relative.to_string()));
    }
    if to_canonical.exists() {
        return Err(ProjectError::AlreadyExists(to_relative.to_string()));
    }
    fs::rename(&from_canonical, &to_canonical).map_err(ProjectError::Rename)
}

/// Rename a directory under docs root. Fails if the source is missing, the
/// destination already exists, or the source is the docs root itself.
pub fn rename_project_dir(
    docs_root: &str,
    from_relative: &str,
    to_relative: &str,
) -> Result<(), ProjectError> {
    validate_relative_name(from_relative)?;
    validate_relative_name(to_relative)?;
    let root = resolve_docs_root(docs_root)?;
    let from_joined = paths::join_relative(&root, from_relative)?;
    let from_canonical = paths::ensure_under(&root, &from_joined)?;
    if from_canonical == root {
        return Err(ProjectError::InvalidName(from_relative.to_string()));
    }
    if !from_canonical.is_dir() {
        return Err(ProjectError::NotFound(from_relative.to_string()));
    }
    let to_joined = paths::join_relative(&root, to_relative)?;
    let to_canonical = paths::ensure_under(&root, &to_joined)?;
    if to_canonical.exists() {
        return Err(ProjectError::AlreadyExists(to_relative.to_string()));
    }
    fs::rename(&from_canonical, &to_canonical).map_err(ProjectError::Rename)
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

    #[test]
    fn delete_file_and_dir_remove_from_tree() {
        let root = temp_dir();
        create_project_dir(root.to_str().unwrap(), "folder").unwrap();
        create_project_file(root.to_str().unwrap(), "folder/note.adoc").unwrap();

        // Delete the file: tree still has the (now empty) folder.
        delete_project_file(root.to_str().unwrap(), "folder/note.adoc").unwrap();
        let tree = list_docs_tree(root.to_str().unwrap()).unwrap();
        assert_eq!(tree.len(), 1);
        assert!(tree[0].is_dir);
        assert_eq!(tree[0].name, "folder");

        // Deleting the file again fails: not found.
        let err = delete_project_file(root.to_str().unwrap(), "folder/note.adoc").unwrap_err();
        assert!(matches!(err, ProjectError::NotFound(_)));

        // Deleting a file path that is actually a dir fails.
        let err = delete_project_file(root.to_str().unwrap(), "folder").unwrap_err();
        assert!(matches!(err, ProjectError::NotFound(_)));

        // Delete the directory: tree is empty.
        delete_project_dir(root.to_str().unwrap(), "folder").unwrap();
        let tree = list_docs_tree(root.to_str().unwrap()).unwrap();
        assert!(tree.is_empty());

        // Deleting the dir again fails: not found.
        let err = delete_project_dir(root.to_str().unwrap(), "folder").unwrap_err();
        assert!(matches!(err, ProjectError::NotFound(_)));

        // Deleting the docs root itself is rejected.
        let err = delete_project_dir(root.to_str().unwrap(), ".").unwrap_err();
        assert!(matches!(err, ProjectError::InvalidName(_)));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rename_file_and_dir_update_tree() {
        let root = temp_dir();
        create_project_dir(root.to_str().unwrap(), "folder").unwrap();
        create_project_file(root.to_str().unwrap(), "folder/note.adoc").unwrap();

        // Rename the file.
        rename_project_file(root.to_str().unwrap(), "folder/note.adoc", "folder/renamed.adoc").unwrap();
        let tree = list_docs_tree(root.to_str().unwrap()).unwrap();
        let children = tree[0].children.as_ref().unwrap();
        assert_eq!(children[0].name, "renamed.adoc");

        // Renaming to an existing name fails.
        create_project_file(root.to_str().unwrap(), "folder/second.adoc").unwrap();
        let err = rename_project_file(
            root.to_str().unwrap(),
            "folder/second.adoc",
            "folder/renamed.adoc",
        )
        .unwrap_err();
        assert!(matches!(err, ProjectError::AlreadyExists(_)));

        // Renaming a missing source fails.
        let err = rename_project_file(
            root.to_str().unwrap(),
            "folder/missing.adoc",
            "folder/other.adoc",
        )
        .unwrap_err();
        assert!(matches!(err, ProjectError::NotFound(_)));

        // Renaming to an unsupported extension fails.
        let err = rename_project_file(
            root.to_str().unwrap(),
            "folder/renamed.adoc",
            "folder/renamed.rs",
        )
        .unwrap_err();
        assert!(matches!(err, ProjectError::UnsupportedFile(_)));

        // Rename the directory.
        rename_project_dir(root.to_str().unwrap(), "folder", "archive").unwrap();
        let tree = list_docs_tree(root.to_str().unwrap()).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "archive");

        // Renaming the docs root is rejected.
        let err = rename_project_dir(root.to_str().unwrap(), ".", "root").unwrap_err();
        assert!(matches!(err, ProjectError::InvalidName(_)));

        // Renaming a file via the dir command fails.
        let err = rename_project_dir(
            root.to_str().unwrap(),
            "archive/renamed.adoc",
            "archive/other.adoc",
        )
        .unwrap_err();
        assert!(matches!(err, ProjectError::NotFound(_)));

        fs::remove_dir_all(&root).ok();
    }
}
