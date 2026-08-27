use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::domain::asciidoc_templates::AsciidocFileTemplate;
use crate::domain::project_config::{
    OpenedProject, ProbeResult, ProjectError, RecentProject, RenameReport, TreeNode,
};
use crate::services::reference_rewrite::{self, RenamedPath};
use crate::services::workspace_index::WorkspaceIndex;
use crate::services::{docs_fs, gitignore, project_open};

#[tauri::command]
pub fn probe_open_path(path: String) -> Result<ProbeResult, String> {
    project_open::probe_open_path(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_project(root: String, docs_root: String) -> Result<OpenedProject, String> {
    project_open::open_project(&root, &docs_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_gitignore_entry(root: String, entry: String) -> Result<(), String> {
    // Prefer the OptMem-aware atlas block when the UI asks to ignore `.atlas`.
    if entry.trim() == ".atlas" || entry.trim() == ".atlas/*" {
        return gitignore::ensure_atlas_gitignore(&root).map_err(|e| e.to_string());
    }
    gitignore::ensure_entry(&root, &entry).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ensure_atlas_gitignore(root: String) -> Result<(), String> {
    gitignore::ensure_atlas_gitignore(&root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_cached_project(root: String) -> Result<OpenedProject, String> {
    project_open::open_cached_project(&root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_project() -> Result<Option<OpenedProject>, String> {
    project_open::get_project().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_saved_repo_root() -> Result<Option<String>, String> {
    project_open::get_saved_repo_root().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_project() -> Result<(), String> {
    project_open::clear_project().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_recent_projects() -> Result<Vec<RecentProject>, String> {
    project_open::list_recent_projects().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_recent_project(root: String) -> Result<(), String> {
    project_open::remove_recent_project(&root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_git_branch(root: String) -> Result<Option<String>, String> {
    Ok(project_open::get_git_branch(&root))
}

#[tauri::command]
pub fn list_docs_tree(docs_root: String) -> Result<Vec<TreeNode>, String> {
    docs_fs::list_docs_tree(&docs_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_project_file(docs_root: String, relative_path: String) -> Result<String, String> {
    docs_fs::read_project_file(&docs_root, &relative_path).map_err(|e| e.to_string())
}

/// Same boundary as `read_project_file`, but a missing file resolves to
/// `Ok(None)` instead of an `Err` string — lets a caller like the assistant's
/// `writeFile` approval diff distinguish "doesn't exist yet, show an empty
/// original" from a real failure (path escape, unsupported extension) without
/// pattern-matching `ProjectError`'s `Display` text on the frontend.
#[tauri::command]
pub fn read_project_file_or_none(
    docs_root: String,
    relative_path: String,
) -> Result<Option<String>, String> {
    match docs_fs::read_project_file(&docs_root, &relative_path) {
        Ok(content) => Ok(Some(content)),
        Err(ProjectError::NotFound(_)) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn resolve_asset_path(docs_root: String, relative_path: String) -> Result<String, String> {
    docs_fs::resolve_asset_path(&docs_root, &relative_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_image_files(docs_root: String) -> Result<Vec<docs_fs::ImageFileEntry>, String> {
    docs_fs::list_image_files(&docs_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_external_file(
    docs_root: String,
    dest_dir_relative: String,
    source_absolute: String,
) -> Result<String, String> {
    docs_fs::import_external_file(&docs_root, &dest_dir_relative, &source_absolute)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_external_text_file(absolute_path: String) -> Result<String, String> {
    docs_fs::read_external_text_file(&absolute_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn write_external_text_file(absolute_path: String, content: String) -> Result<(), String> {
    docs_fs::write_external_text_file(&absolute_path, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn write_project_file(
    docs_root: String,
    relative_path: String,
    content: String,
) -> Result<(), String> {
    docs_fs::write_project_file(&docs_root, &relative_path, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_project_file(docs_root: String, relative_path: String) -> Result<(), String> {
    docs_fs::create_project_file(&docs_root, &relative_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_project_file_from_template(
    docs_root: String,
    relative_path: String,
    template: Option<AsciidocFileTemplate>,
) -> Result<(), String> {
    docs_fs::create_project_file_from_template(&docs_root, &relative_path, template)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_rest_endpoint_folder(
    docs_root: String,
    relative_path: String,
    method_name: String,
) -> Result<(), String> {
    docs_fs::create_rest_endpoint_folder(&docs_root, &relative_path, &method_name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_project_dir(docs_root: String, relative_path: String) -> Result<(), String> {
    docs_fs::create_project_dir(&docs_root, &relative_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_project_file(docs_root: String, relative_path: String) -> Result<(), String> {
    docs_fs::delete_project_file(&docs_root, &relative_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_project_dir(docs_root: String, relative_path: String) -> Result<(), String> {
    docs_fs::delete_project_dir(&docs_root, &relative_path, true).map_err(|e| e.to_string())
}

/// Resolves the index's repo root and `docs_root`'s suffix relative to it
/// together, so callers get both or neither in one match — `docs_root_suffix`
/// itself now lives in `services::reference_rewrite` (shared with
/// `services::ai_tools::tools::move_path::move_path`, which doesn't have a `WorkspaceIndex`
/// handy the way commands here do, only `ToolScope::repo_root` directly).
fn docs_root_suffix_and_repo_root(index: &WorkspaceIndex, docs_root: &str) -> Option<(PathBuf, String)> {
    let repo_root = index.repo_root()?;
    let suffix = reference_rewrite::docs_root_suffix(&repo_root, docs_root)?;
    Some((repo_root, suffix))
}

#[tauri::command]
pub fn rename_project_file(
    index: State<'_, Arc<WorkspaceIndex>>,
    docs_root: String,
    from_relative: String,
    to_relative: String,
) -> Result<RenameReport, String> {
    let resolved = docs_root_suffix_and_repo_root(&index, &docs_root);
    let renamed: Vec<RenamedPath> = match &resolved {
        Some((_, suffix)) => vec![RenamedPath {
            old: reference_rewrite::to_repo_relative(suffix, &from_relative),
            new: reference_rewrite::to_repo_relative(suffix, &to_relative),
        }],
        None => Vec::new(),
    };

    let report = match &resolved {
        Some((repo_root, suffix)) => {
            let rewritten = reference_rewrite::rewrite_references(&index, repo_root, &renamed)
                .map_err(|e| e.to_string())?;
            reference_rewrite::into_report(suffix, rewritten)
        }
        None => RenameReport::default(),
    };

    docs_fs::rename_project_file(&docs_root, &from_relative, &to_relative)
        .map_err(|e| e.to_string())?;

    // Keep the renamed document's own index row in sync immediately,
    // rather than only once the async file-watcher gets to it — mirrors
    // `services::ai_tools::tools::move_path::move_path`'s identical fix for the AI-driven
    // path. Best-effort: a rename that succeeded on disk must not be
    // reported as failed just because this lagged/errored.
    if let Some((repo_root, _)) = &resolved {
        for pair in &renamed {
            let _ = index.rename_document(repo_root.join(&pair.old), repo_root.join(&pair.new));
        }
    }

    Ok(report)
}

#[tauri::command]
pub fn rename_project_dir(
    index: State<'_, Arc<WorkspaceIndex>>,
    docs_root: String,
    from_relative: String,
    to_relative: String,
) -> Result<RenameReport, String> {
    let resolved = docs_root_suffix_and_repo_root(&index, &docs_root);
    let renamed: Vec<RenamedPath> = match &resolved {
        Some((_, suffix)) => {
            let old = reference_rewrite::to_repo_relative(suffix, &from_relative);
            let new = reference_rewrite::to_repo_relative(suffix, &to_relative);
            reference_rewrite::renamed_paths_for_dir_move(&index, &old, &new)
        }
        None => Vec::new(),
    };

    let report = match &resolved {
        Some((repo_root, suffix)) => {
            let rewritten = reference_rewrite::rewrite_references(&index, repo_root, &renamed)
                .map_err(|e| e.to_string())?;
            reference_rewrite::into_report(suffix, rewritten)
        }
        None => RenameReport::default(),
    };

    docs_fs::rename_project_dir(&docs_root, &from_relative, &to_relative)
        .map_err(|e| e.to_string())?;

    // See `rename_project_file`'s matching comment.
    if let Some((repo_root, _)) = &resolved {
        for pair in &renamed {
            let _ = index.rename_document(repo_root.join(&pair.old), repo_root.join(&pair.new));
        }
    }

    Ok(report)
}

#[tauri::command]
pub fn copy_project_file(
    docs_root: String,
    from_relative: String,
    to_relative: String,
) -> Result<(), String> {
    docs_fs::copy_project_file(&docs_root, &from_relative, &to_relative)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn copy_project_dir(
    docs_root: String,
    from_relative: String,
    to_relative: String,
) -> Result<(), String> {
    docs_fs::copy_project_dir(&docs_root, &from_relative, &to_relative)
        .map_err(|e| e.to_string())
}

/// Result of checking whether a path exists on disk.
#[derive(serde::Serialize)]
pub struct PathExistsResult {
    pub exists: bool,
    pub is_dir: bool,
    pub is_non_empty: bool,
}

#[tauri::command]
pub fn check_path_exists(path: String) -> Result<PathExistsResult, String> {
    let p = std::path::Path::new(&path);
    Ok(PathExistsResult {
        exists: p.exists(),
        is_dir: p.is_dir(),
        is_non_empty: p
            .read_dir()
            .ok()
            .map(|mut d| d.next().is_some())
            .unwrap_or(false),
    })
}
