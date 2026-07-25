use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::paths;
use crate::domain::project_config::{ProjectError, TreeNode};
use crate::domain::supported_files::is_supported_file;

/// List a filtered tree of supported files under `docs_root`.
/// Directories that contain no supported files (recursively) are omitted.
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
            if children.is_empty() {
                continue;
            }
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

fn resolve_docs_root(docs_root: &str) -> Result<PathBuf, ProjectError> {
    let path = Path::new(docs_root);
    if !path.is_dir() {
        return Err(ProjectError::NotADirectory(docs_root.to_string()));
    }
    path.canonicalize().map_err(ProjectError::Canonicalize)
}
