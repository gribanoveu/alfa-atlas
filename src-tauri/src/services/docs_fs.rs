use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::domain::asciidoc_templates::{AsciidocFileTemplate, SEQUENCE_DIAGRAM_TEMPLATE};
use crate::domain::paths;
use crate::domain::project_config::{ProjectError, TreeNode};
use crate::domain::supported_files::{is_docs_tree_file, is_image_asset, is_supported_file};
use crate::infra::workspace_scanner;

/// Docs-root-relative image file for `image::` completions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageFileEntry {
    pub relative_path: String,
    pub file_name: String,
}

/// List a filtered tree of supported files **and image assets** under
/// `docs_root` for the sidebar. Empty directories are included so newly
/// created folders appear in the UI. AI Docs-only listing uses
/// `list_docs_tree_scoped` instead (no images).
pub fn list_docs_tree(docs_root: &str) -> Result<Vec<TreeNode>, ProjectError> {
    let root = PathBuf::from(docs_root);
    if !root.is_dir() {
        return Err(ProjectError::NotADirectory(docs_root.to_string()));
    }
    let root = root.canonicalize().map_err(ProjectError::Canonicalize)?;
    build_dir_children(&root, &root, None, true)
}

/// Same walk as `list_docs_tree`, but starting at `dir` (which may be a
/// subdirectory of `docs_root`, already validated by the caller) instead of
/// always walking the whole `docs_root`, and capped at `max_depth` levels
/// below `dir`. Paths in the returned tree stay relative to `docs_root`,
/// not `dir` — used by `services::ai_tools::tools::list_files::list_docs_only` so a scoped
/// `listFiles` call still returns paths the caller can round-trip into
/// `readFile`/`writeFile` unchanged.
///
/// Image assets are **excluded** here so the LLM does not see binary files.
pub fn list_docs_tree_scoped(
    docs_root: &Path,
    dir: &Path,
    max_depth: Option<u32>,
) -> Result<Vec<TreeNode>, ProjectError> {
    let docs_root = docs_root.canonicalize().map_err(ProjectError::Canonicalize)?;
    let dir = dir.canonicalize().map_err(ProjectError::Canonicalize)?;
    if !dir.is_dir() {
        return Err(ProjectError::NotADirectory(dir.display().to_string()));
    }
    build_dir_children(&docs_root, &dir, max_depth, false)
}

fn build_dir_children(
    docs_root: &Path,
    dir: &Path,
    remaining_depth: Option<u32>,
    include_images: bool,
) -> Result<Vec<TreeNode>, ProjectError> {
    if remaining_depth == Some(0) {
        return Ok(Vec::new());
    }

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
            let children = build_dir_children(
                docs_root,
                &path,
                remaining_depth.map(|d| d - 1),
                include_images,
            )?;
            let rel = paths::relative_to(docs_root, &path)?;
            nodes.push(TreeNode {
                name,
                path: rel,
                is_dir: true,
                children: Some(children),
            });
        } else if file_type.is_file() {
            let path_str = path.to_string_lossy();
            let visible = if include_images {
                is_docs_tree_file(&path_str)
            } else {
                is_supported_file(&path_str)
            };
            if !visible {
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

/// List image assets under `docs_root` (gitignore-aware). Paths are
/// docs-root-relative with `/` separators — suitable for `image::`
/// completions after `relativizeToDocument` on the frontend.
pub fn list_image_files(docs_root: &str) -> Result<Vec<ImageFileEntry>, ProjectError> {
    let root = resolve_docs_root(docs_root)?;
    let scanned = workspace_scanner::scan_all(&root).map_err(|e| match e {
        crate::domain::workspace_index::WorkspaceIndexError::Io(err) => ProjectError::Read(err),
        other => ProjectError::Message(other.to_string()),
    })?;

    let mut out = Vec::new();
    for file in scanned {
        let path_str = file.path.to_string_lossy();
        if !is_image_asset(&path_str) {
            continue;
        }
        let relative = paths::relative_to(&root, &file.path)?;
        let file_name = file
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| relative.clone());
        out.push(ImageFileEntry {
            relative_path: relative,
            file_name,
        });
    }
    out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(out)
}

/// Resolve an asset (e.g. image) referenced from a docs file into a
/// canonical absolute filesystem path. Mirrors `read_project_file`'s
/// path validation (`join_relative` rejects `..`, `ensure_under`
/// canonicalizes and confirms containment under `docs_root`), but
/// intentionally skips the `is_supported_file` filter — image
/// extensions (.png/.jpg/...) are not in the supported-doc list.
///
/// The frontend turns the returned path into a WebView-loadable URL via
/// Tauri's `convertFileSrc`.
pub fn resolve_asset_path(docs_root: &str, relative_path: &str) -> Result<String, ProjectError> {
    let root = resolve_docs_root(docs_root)?;
    let joined = paths::join_relative(&root, relative_path)?;
    let canonical = paths::ensure_under(&root, &joined)?;
    if !canonical.is_file() {
        return Err(ProjectError::NotFound(relative_path.to_string()));
    }
    Ok(canonical.to_string_lossy().into_owned())
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

/// Create a new supported file with the given initial content. Fails if the
/// path already exists.
pub fn create_project_file_with_content(
    docs_root: &str,
    relative_path: &str,
    content: &str,
) -> Result<(), ProjectError> {
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
    fs::write(&target, content).map_err(ProjectError::Write)
}

/// Create a new empty supported file. Fails if the path already exists.
pub fn create_project_file(docs_root: &str, relative_path: &str) -> Result<(), ProjectError> {
    create_project_file_with_content(docs_root, relative_path, "")
}

/// Create a file populated from an AsciiDoc template (or empty when `template`
/// is `None`).
pub fn create_project_file_from_template(
    docs_root: &str,
    relative_path: &str,
    template: Option<AsciidocFileTemplate>,
) -> Result<(), ProjectError> {
    let content = template.map(AsciidocFileTemplate::content).unwrap_or("");
    create_project_file_with_content(docs_root, relative_path, content)
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

/// Create a new folder populated with the REST-endpoint template set:
/// `{method_name}.adoc` (from the method template) plus `request.adoc`,
/// `response.adoc`, and `{method_name}.puml` copied from
/// `src/templates/asciidoc/rest-endpoint`.
pub fn create_rest_endpoint_folder(
    docs_root: &str,
    relative_path: &str,
    method_name: &str,
) -> Result<(), ProjectError> {
    create_project_dir(docs_root, relative_path)?;

    let child_path = |name: &str| -> String {
        if relative_path.is_empty() || relative_path == "." {
            name.to_string()
        } else {
            format!("{relative_path}/{name}")
        }
    };

    let method_content = AsciidocFileTemplate::Method
        .content()
        .replace("sequence_diagramm", method_name);
    create_project_file_with_content(
        docs_root,
        &child_path(&format!("{method_name}.adoc")),
        &method_content,
    )?;
    create_project_file_from_template(
        docs_root,
        &child_path("request.adoc"),
        Some(AsciidocFileTemplate::Request),
    )?;
    create_project_file_from_template(
        docs_root,
        &child_path("response.adoc"),
        Some(AsciidocFileTemplate::Response),
    )?;
    create_project_file_with_content(
        docs_root,
        &child_path(&format!("{method_name}.puml")),
        SEQUENCE_DIAGRAM_TEMPLATE,
    )?;

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
/// or if the target is the docs root itself. `recursive: false` additionally
/// refuses a non-empty directory (`ProjectError::DirectoryNotEmpty`) rather
/// than silently deleting its contents — callers that already confirm
/// "delete everything inside" client-side (the editor's own delete UI, via
/// `commands::project::delete_project_dir`) pass `true`.
pub fn delete_project_dir(
    docs_root: &str,
    relative_path: &str,
    recursive: bool,
) -> Result<(), ProjectError> {
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
    if !recursive && fs::read_dir(&canonical).map_err(ProjectError::Delete)?.next().is_some() {
        return Err(ProjectError::DirectoryNotEmpty(relative_path.to_string()));
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
    if !is_docs_tree_file(&to_canonical.to_string_lossy()) {
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

/// Copy a file under docs root to a new path. Fails if the source is
/// missing, the destination already exists, or the new name is not a
/// supported file type.
pub fn copy_project_file(
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
    if !is_docs_tree_file(&to_canonical.to_string_lossy()) {
        return Err(ProjectError::UnsupportedFile(to_relative.to_string()));
    }
    if to_canonical.exists() {
        return Err(ProjectError::AlreadyExists(to_relative.to_string()));
    }
    fs::copy(&from_canonical, &to_canonical)
        .map(|_| ())
        .map_err(ProjectError::Copy)
}

/// Copy a directory under docs root (recursively) to a new path. Fails if
/// the source is missing, the destination already exists, the source is the
/// docs root itself, or the destination is nested inside the source (which
/// would make the recursive copy walk its own output).
pub fn copy_project_dir(
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
    if to_joined.exists() {
        return Err(ProjectError::AlreadyExists(to_relative.to_string()));
    }
    if to_joined.starts_with(&from_canonical) {
        return Err(ProjectError::PathEscape(to_relative.to_string()));
    }

    copy_dir_recursive(&from_canonical, &to_joined)?;

    let to_canonical = to_joined.canonicalize().map_err(ProjectError::Canonicalize)?;
    if !to_canonical.starts_with(&root) {
        let _ = fs::remove_dir_all(&to_canonical);
        return Err(ProjectError::PathEscape(to_relative.to_string()));
    }
    Ok(())
}

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<(), ProjectError> {
    fs::create_dir_all(to).map_err(ProjectError::CreateDir)?;
    for entry in fs::read_dir(from).map_err(ProjectError::Read)? {
        let entry = entry.map_err(ProjectError::Read)?;
        let file_type = entry.file_type().map_err(ProjectError::Read)?;
        let dest = to.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &dest).map_err(ProjectError::Copy)?;
        }
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

/// Resolve a docs-root destination directory. `"."` / empty means the docs root itself.
fn resolve_dest_dir(docs_root: &Path, dest_dir_relative: &str) -> Result<PathBuf, ProjectError> {
    let trimmed = dest_dir_relative.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Ok(docs_root.to_path_buf());
    }
    validate_relative_name(trimmed)?;
    let joined = paths::join_relative(docs_root, trimmed)?;
    let canonical = paths::ensure_under(docs_root, &joined)?;
    if !canonical.is_dir() {
        return Err(ProjectError::NotADirectory(trimmed.to_string()));
    }
    Ok(canonical)
}

/// Split `file_name` into `(stem, extension_without_dot)`. `"logo.png"` → `("logo", "png")`.
fn split_file_name(file_name: &str) -> (&str, &str) {
    match file_name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && !stem.contains('/') => {
            (stem, ext)
        }
        _ => (file_name, ""),
    }
}

/// Pick a non-colliding path under `dest_dir`: `name.ext`, then `name (1).ext`, …
fn unique_dest_path(dest_dir: &Path, file_name: &str) -> PathBuf {
    let candidate = dest_dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = split_file_name(file_name);
    for n in 1..10_000 {
        let name = if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        let candidate = dest_dir.join(&name);
        if !candidate.exists() {
            return candidate;
        }
    }
    dest_dir.join(file_name)
}

/// Copy an OS file into `docs_root/dest_dir_relative/`, generating a unique
/// name on collision. Any extension is allowed (tree listing still filters).
/// Returns the docs-root-relative destination path.
pub fn import_external_file(
    docs_root: &str,
    dest_dir_relative: &str,
    source_absolute: &str,
) -> Result<String, ProjectError> {
    let root = resolve_docs_root(docs_root)?;
    let dest_dir = resolve_dest_dir(&root, dest_dir_relative)?;

    let source = Path::new(source_absolute);
    let source_canonical = source.canonicalize().map_err(ProjectError::Canonicalize)?;
    if !source_canonical.is_file() {
        return Err(ProjectError::NotFound(source_absolute.to_string()));
    }

    let file_name = source_canonical
        .file_name()
        .ok_or_else(|| ProjectError::InvalidName(source_absolute.to_string()))?
        .to_string_lossy()
        .into_owned();

    let target = unique_dest_path(&dest_dir, &file_name);
    fs::copy(&source_canonical, &target).map_err(ProjectError::Copy)?;

    let target_canonical = target.canonicalize().map_err(ProjectError::Canonicalize)?;
    if !target_canonical.starts_with(&root) {
        let _ = fs::remove_file(&target_canonical);
        return Err(ProjectError::PathEscape(target.display().to_string()));
    }

    paths::relative_to(&root, &target_canonical)
}

/// Read a supported text file from an absolute OS path (outside docs root).
pub fn read_external_text_file(absolute_path: &str) -> Result<String, ProjectError> {
    if !is_supported_file(absolute_path) {
        return Err(ProjectError::UnsupportedFile(absolute_path.to_string()));
    }
    let canonical = Path::new(absolute_path)
        .canonicalize()
        .map_err(ProjectError::Canonicalize)?;
    if !canonical.is_file() {
        return Err(ProjectError::NotFound(absolute_path.to_string()));
    }
    if !is_supported_file(&canonical.to_string_lossy()) {
        return Err(ProjectError::UnsupportedFile(absolute_path.to_string()));
    }
    fs::read_to_string(&canonical).map_err(ProjectError::Read)
}

/// Write a supported text file at an absolute OS path (outside docs root).
pub fn write_external_text_file(absolute_path: &str, content: &str) -> Result<(), ProjectError> {
    if !is_supported_file(absolute_path) {
        return Err(ProjectError::UnsupportedFile(absolute_path.to_string()));
    }
    let path = Path::new(absolute_path);
    if path.exists() {
        let canonical = path.canonicalize().map_err(ProjectError::Canonicalize)?;
        if !canonical.is_file() {
            return Err(ProjectError::NotFound(absolute_path.to_string()));
        }
        if !is_supported_file(&canonical.to_string_lossy()) {
            return Err(ProjectError::UnsupportedFile(absolute_path.to_string()));
        }
        return fs::write(&canonical, content).map_err(ProjectError::Write);
    }
    fs::write(path, content).map_err(ProjectError::Write)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        // Nanosecond timestamps alone can collide between parallel test
        // threads on coarser clocks; a per-process counter guarantees
        // uniqueness regardless of clock resolution.
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("alfa-atlas-fs-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Regression test for `paths::ensure_under`'s old behavior of
    /// canonicalizing only the immediate parent: `writeFile`'s tool
    /// description promises missing parent directories are created
    /// automatically, but before that fix this failed as soon as *more*
    /// than one level of the path was missing (e.g. writing into a brand
    /// new subtree in one call), rather than only when a single directory
    /// needed creating.
    #[test]
    fn write_project_file_creates_several_missing_nested_directories_at_once() {
        let root = temp_dir();

        write_project_file(root.to_str().unwrap(), "brand/new/dir/note.adoc", "hello").unwrap();
        assert_eq!(
            fs::read_to_string(root.join("brand/new/dir/note.adoc")).unwrap(),
            "hello"
        );

        fs::remove_dir_all(&root).ok();
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
    fn create_rest_endpoint_folder_populates_template_files() {
        let root = temp_dir();
        create_rest_endpoint_folder(root.to_str().unwrap(), "getUserProfile", "getUserProfile")
            .unwrap();

        let tree = list_docs_tree(root.to_str().unwrap()).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "getUserProfile");
        assert!(tree[0].is_dir);
        let mut names: Vec<&str> = tree[0]
            .children
            .as_ref()
            .unwrap()
            .iter()
            .map(|n| n.name.as_str())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "getUserProfile.adoc",
                "getUserProfile.puml",
                "request.adoc",
                "response.adoc",
            ]
        );

        let method_content =
            read_project_file(root.to_str().unwrap(), "getUserProfile/getUserProfile.adoc")
                .unwrap();
        assert!(method_content.contains("Метод Template"));
        assert!(method_content.contains("include::getUserProfile.puml[]"));

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

        // Delete the (now empty) directory: tree is empty. `recursive: false`
        // is fine here since the folder has nothing left in it.
        delete_project_dir(root.to_str().unwrap(), "folder", false).unwrap();
        let tree = list_docs_tree(root.to_str().unwrap()).unwrap();
        assert!(tree.is_empty());

        // Deleting the dir again fails: not found.
        let err = delete_project_dir(root.to_str().unwrap(), "folder", false).unwrap_err();
        assert!(matches!(err, ProjectError::NotFound(_)));

        // Deleting the docs root itself is rejected.
        let err = delete_project_dir(root.to_str().unwrap(), ".", true).unwrap_err();
        assert!(matches!(err, ProjectError::InvalidName(_)));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn delete_project_dir_refuses_a_non_empty_directory_by_default() {
        let root = temp_dir();
        create_project_dir(root.to_str().unwrap(), "folder").unwrap();
        create_project_file(root.to_str().unwrap(), "folder/note.adoc").unwrap();

        let err = delete_project_dir(root.to_str().unwrap(), "folder", false).unwrap_err();
        assert!(matches!(err, ProjectError::DirectoryNotEmpty(_)));
        // Nothing was actually deleted.
        assert!(root.join("folder/note.adoc").exists());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn delete_project_dir_recursive_true_deletes_a_non_empty_directory() {
        let root = temp_dir();
        create_project_dir(root.to_str().unwrap(), "folder").unwrap();
        create_project_file(root.to_str().unwrap(), "folder/note.adoc").unwrap();

        delete_project_dir(root.to_str().unwrap(), "folder", true).unwrap();
        assert!(!root.join("folder").exists());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_asset_path_returns_canonical_for_existing_file() {
        let root = temp_dir();
        create_project_dir(root.to_str().unwrap(), "img").unwrap();
        let img = root.join("img").join("screenshot.png");
        fs::write(&img, b"png-bytes").unwrap();

        let resolved = resolve_asset_path(root.to_str().unwrap(), "img/screenshot.png").unwrap();
        assert_eq!(PathBuf::from(&resolved).canonicalize().unwrap(), img.canonicalize().unwrap());

        // Missing file → NotFound.
        let err = resolve_asset_path(root.to_str().unwrap(), "img/missing.png").unwrap_err();
        assert!(matches!(err, ProjectError::NotFound(_)));

        // Path traversal rejected by join_relative.
        let err = resolve_asset_path(root.to_str().unwrap(), "../outside.png").unwrap_err();
        assert!(matches!(err, ProjectError::PathEscape(_)));

        // Directories are not valid assets.
        let err = resolve_asset_path(root.to_str().unwrap(), "img").unwrap_err();
        assert!(matches!(err, ProjectError::NotFound(_)));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn list_image_files_returns_docs_relative_image_assets() {
        let root = temp_dir();
        create_project_dir(root.to_str().unwrap(), "img").unwrap();
        create_project_dir(root.to_str().unwrap(), "img/icons").unwrap();
        fs::write(root.join("img/logo.png"), b"png").unwrap();
        fs::write(root.join("img/icons/a.SVG"), b"<svg/>").unwrap();
        fs::write(root.join("readme.adoc"), "= Hi\n").unwrap();
        fs::write(root.join("img/notes.txt"), "x").unwrap();

        let listed = list_image_files(root.to_str().unwrap()).unwrap();
        let paths: Vec<&str> = listed.iter().map(|e| e.relative_path.as_str()).collect();
        assert_eq!(paths, vec!["img/icons/a.SVG", "img/logo.png"]);
        assert_eq!(listed[1].file_name, "logo.png");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn list_docs_tree_includes_images_scoped_excludes_them() {
        let root = temp_dir();
        create_project_file(root.to_str().unwrap(), "note.adoc").unwrap();
        fs::write(root.join("logo.png"), b"png").unwrap();

        let ui = list_docs_tree(root.to_str().unwrap()).unwrap();
        let ui_names: Vec<&str> = ui.iter().map(|n| n.name.as_str()).collect();
        assert!(ui_names.contains(&"note.adoc"));
        assert!(ui_names.contains(&"logo.png"));

        let scoped = list_docs_tree_scoped(&root, &root, None).unwrap();
        let scoped_names: Vec<&str> = scoped.iter().map(|n| n.name.as_str()).collect();
        assert!(scoped_names.contains(&"note.adoc"));
        assert!(!scoped_names.contains(&"logo.png"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rename_and_copy_allow_image_destinations() {
        let root = temp_dir();
        fs::write(root.join("a.png"), b"png").unwrap();

        rename_project_file(root.to_str().unwrap(), "a.png", "b.png").unwrap();
        assert!(root.join("b.png").is_file());
        assert!(!root.join("a.png").exists());

        copy_project_file(root.to_str().unwrap(), "b.png", "c.png").unwrap();
        assert!(root.join("c.png").is_file());

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

    #[test]
    fn copy_file_duplicates_content() {
        let root = temp_dir();
        create_project_dir(root.to_str().unwrap(), "folder").unwrap();
        create_project_file(root.to_str().unwrap(), "folder/note.adoc").unwrap();
        fs::write(root.join("folder/note.adoc"), "hello").unwrap();

        copy_project_file(
            root.to_str().unwrap(),
            "folder/note.adoc",
            "folder/note copy.adoc",
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("folder/note copy.adoc")).unwrap(),
            "hello"
        );
        // Original untouched.
        assert_eq!(
            fs::read_to_string(root.join("folder/note.adoc")).unwrap(),
            "hello"
        );

        // Copying onto an existing destination fails.
        let err = copy_project_file(
            root.to_str().unwrap(),
            "folder/note.adoc",
            "folder/note copy.adoc",
        )
        .unwrap_err();
        assert!(matches!(err, ProjectError::AlreadyExists(_)));

        // Copying a missing source fails.
        let err = copy_project_file(
            root.to_str().unwrap(),
            "folder/missing.adoc",
            "folder/other.adoc",
        )
        .unwrap_err();
        assert!(matches!(err, ProjectError::NotFound(_)));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn copy_dir_duplicates_tree_recursively() {
        let root = temp_dir();
        create_project_dir(root.to_str().unwrap(), "folder").unwrap();
        create_project_dir(root.to_str().unwrap(), "folder/nested").unwrap();
        create_project_file(root.to_str().unwrap(), "folder/note.adoc").unwrap();
        create_project_file(root.to_str().unwrap(), "folder/nested/inner.adoc").unwrap();
        fs::write(root.join("folder/note.adoc"), "top").unwrap();
        fs::write(root.join("folder/nested/inner.adoc"), "deep").unwrap();

        copy_project_dir(root.to_str().unwrap(), "folder", "folder copy").unwrap();

        assert_eq!(
            fs::read_to_string(root.join("folder copy/note.adoc")).unwrap(),
            "top"
        );
        assert_eq!(
            fs::read_to_string(root.join("folder copy/nested/inner.adoc")).unwrap(),
            "deep"
        );
        // Original untouched.
        assert!(root.join("folder/note.adoc").exists());

        // Copying onto an existing destination fails.
        let err = copy_project_dir(root.to_str().unwrap(), "folder", "folder copy").unwrap_err();
        assert!(matches!(err, ProjectError::AlreadyExists(_)));

        // Copying the docs root itself is rejected.
        let err = copy_project_dir(root.to_str().unwrap(), ".", "root copy").unwrap_err();
        assert!(matches!(err, ProjectError::InvalidName(_)));

        // Copying a directory into its own subtree is rejected (would recurse
        // into its own output).
        let err =
            copy_project_dir(root.to_str().unwrap(), "folder", "folder/self-copy").unwrap_err();
        assert!(matches!(err, ProjectError::PathEscape(_)));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn list_docs_tree_scoped_limits_recursion_to_max_depth() {
        let root = temp_dir();
        create_project_file(root.to_str().unwrap(), "a.adoc").unwrap();
        create_project_dir(root.to_str().unwrap(), "sub").unwrap();
        create_project_file(root.to_str().unwrap(), "sub/b.adoc").unwrap();
        create_project_dir(root.to_str().unwrap(), "sub/deeper").unwrap();
        create_project_file(root.to_str().unwrap(), "sub/deeper/c.adoc").unwrap();

        // depth 1: `a.adoc` and `sub` itself, but `sub`'s children are empty.
        let tree = list_docs_tree_scoped(&root, &root, Some(1)).unwrap();
        let mut names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["a.adoc", "sub"]);
        let sub = tree.iter().find(|n| n.name == "sub").unwrap();
        assert!(sub.children.as_ref().unwrap().is_empty());

        // depth 0: nothing at all.
        let tree = list_docs_tree_scoped(&root, &root, Some(0)).unwrap();
        assert!(tree.is_empty());

        // Unlimited: everything, including the deepest file.
        let tree = list_docs_tree_scoped(&root, &root, None).unwrap();
        let sub = tree.iter().find(|n| n.name == "sub").unwrap();
        let sub_children = sub.children.as_ref().unwrap();
        let deeper = sub_children.iter().find(|n| n.name == "deeper").unwrap();
        assert_eq!(deeper.children.as_ref().unwrap()[0].name, "c.adoc");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn list_docs_tree_scoped_starts_from_given_subdir_but_relativizes_to_docs_root() {
        let root = temp_dir();
        create_project_dir(root.to_str().unwrap(), "sub").unwrap();
        create_project_file(root.to_str().unwrap(), "sub/x.adoc").unwrap();
        create_project_dir(root.to_str().unwrap(), "other").unwrap();
        create_project_file(root.to_str().unwrap(), "other/y.adoc").unwrap();

        let tree = list_docs_tree_scoped(&root, &root.join("sub"), None).unwrap();
        assert_eq!(tree.len(), 1);
        // Relative to `docs_root`, not to the `sub` walk-start — round-trips
        // into `readFile`/`writeFile` the same way a root-wide listing would.
        assert_eq!(tree[0].path, "sub/x.adoc");
        assert_eq!(tree[0].name, "x.adoc");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn import_external_file_copies_and_avoids_name_collisions() {
        let root = temp_dir();
        create_project_dir(root.to_str().unwrap(), "img").unwrap();

        let outside = temp_dir();
        let src = outside.join("logo.png");
        fs::write(&src, b"png-bytes").unwrap();

        let rel = import_external_file(
            root.to_str().unwrap(),
            "img",
            src.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(rel, "img/logo.png");
        assert_eq!(fs::read(root.join("img/logo.png")).unwrap(), b"png-bytes");

        let rel2 = import_external_file(
            root.to_str().unwrap(),
            "img",
            src.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(rel2, "img/logo (1).png");
        assert!(root.join("img/logo (1).png").is_file());

        let tree = list_docs_tree(root.to_str().unwrap()).unwrap();
        let img = tree.iter().find(|n| n.name == "img").unwrap();
        let names: Vec<&str> = img
            .children
            .as_ref()
            .unwrap()
            .iter()
            .map(|n| n.name.as_str())
            .collect();
        assert!(names.contains(&"logo.png"));
        assert!(names.contains(&"logo (1).png"));

        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn import_external_into_docs_root_and_read_write_external_text() {
        let root = temp_dir();
        let outside = temp_dir();
        let src = outside.join("note.md");
        fs::write(&src, "# hi\n").unwrap();

        let rel = import_external_file(
            root.to_str().unwrap(),
            ".",
            src.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(rel, "note.md");

        let content = read_external_text_file(src.to_str().unwrap()).unwrap();
        assert_eq!(content, "# hi\n");
        write_external_text_file(src.to_str().unwrap(), "# bye\n").unwrap();
        assert_eq!(fs::read_to_string(&src).unwrap(), "# bye\n");

        let png = outside.join("x.png");
        fs::write(&png, b"x").unwrap();
        let err = read_external_text_file(png.to_str().unwrap()).unwrap_err();
        assert!(matches!(err, ProjectError::UnsupportedFile(_)));

        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&outside).ok();
    }
}
