use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::domain::asciidoc_templates::AsciidocFileTemplate;
use crate::domain::paths;
use crate::domain::project_config::{
    OpenedProject, ProbeResult, ProjectError, RecentProject, RenameReport, TreeNode,
    UpdatedReference,
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
    gitignore::ensure_entry(&root, &entry).map_err(|e| e.to_string())
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

/// Resolves `docs_root`'s path relative to the index's own repo root (e.g.
/// `"src/docs/asciidoc"`), so docs-root-relative command params can be
/// converted into the repo-root-relative keys the workspace index uses.
/// Returns `None` (rather than an error) when the index isn't open or
/// `docs_root` can't be resolved under it — callers treat that as "nothing
/// to cascade," not as a reason to fail the rename itself.
fn docs_root_suffix(index: &WorkspaceIndex, docs_root: &str) -> Option<String> {
    let repo_root = index.repo_root()?;
    let suffix = paths::relative_to(&repo_root, &PathBuf::from(docs_root)).ok()?;
    Some(if suffix == "." { String::new() } else { suffix })
}

fn to_repo_relative(suffix: &str, docs_relative: &str) -> String {
    if suffix.is_empty() {
        docs_relative.to_string()
    } else {
        format!("{suffix}/{docs_relative}")
    }
}

fn to_docs_relative(suffix: &str, repo_relative: &str) -> Option<String> {
    if suffix.is_empty() {
        return Some(repo_relative.to_string());
    }
    repo_relative
        .strip_prefix(&format!("{suffix}/"))
        .map(str::to_string)
}

/// Cascades a rename's rewritten-reference report (repo-relative) into a
/// docs-root-relative `RenameReport` for the frontend. Files outside
/// `docs_root` (e.g. under `_external/`, if that lives outside it) were
/// still correctly rewritten on disk — they're just not reported as
/// reloadable open tabs, since `editor.openFile` only knows docs-relative
/// paths.
fn into_report(suffix: &str, rewritten: Vec<reference_rewrite::RewrittenFile>) -> RenameReport {
    RenameReport {
        updated_files: rewritten
            .into_iter()
            .filter_map(|f| {
                to_docs_relative(suffix, &f.repo_relative_path).map(|docs_relative_path| {
                    UpdatedReference {
                        docs_relative_path,
                        count: f.count,
                    }
                })
            })
            .collect(),
    }
}

#[tauri::command]
pub fn rename_project_file(
    index: State<'_, Arc<WorkspaceIndex>>,
    docs_root: String,
    from_relative: String,
    to_relative: String,
) -> Result<RenameReport, String> {
    let report = match docs_root_suffix(&index, &docs_root) {
        Some(suffix) => {
            let old = to_repo_relative(&suffix, &from_relative);
            let new = to_repo_relative(&suffix, &to_relative);
            let repo_root = index.repo_root().expect("suffix implies repo_root is set");
            let renamed = [RenamedPath { old, new }];
            let rewritten = reference_rewrite::rewrite_references(&index, &repo_root, &renamed)
                .map_err(|e| e.to_string())?;
            into_report(&suffix, rewritten)
        }
        None => RenameReport::default(),
    };

    docs_fs::rename_project_file(&docs_root, &from_relative, &to_relative)
        .map_err(|e| e.to_string())?;
    Ok(report)
}

#[tauri::command]
pub fn rename_project_dir(
    index: State<'_, Arc<WorkspaceIndex>>,
    docs_root: String,
    from_relative: String,
    to_relative: String,
) -> Result<RenameReport, String> {
    let report = match docs_root_suffix(&index, &docs_root) {
        Some(suffix) => {
            let old = to_repo_relative(&suffix, &from_relative);
            let new = to_repo_relative(&suffix, &to_relative);
            let repo_root = index.repo_root().expect("suffix implies repo_root is set");
            let renamed = reference_rewrite::renamed_paths_for_dir_move(&index, &old, &new);
            let rewritten = reference_rewrite::rewrite_references(&index, &repo_root, &renamed)
                .map_err(|e| e.to_string())?;
            into_report(&suffix, rewritten)
        }
        None => RenameReport::default(),
    };

    docs_fs::rename_project_dir(&docs_root, &from_relative, &to_relative)
        .map_err(|e| e.to_string())?;
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
