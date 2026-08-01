use std::path::Path;

use crate::domain::openapi::{OpenApiBundleResult, SpecsRepoInfo};
use crate::services::{general_prefs, openapi};

#[tauri::command]
pub fn detect_specs_repo(repo_root: String) -> Result<Option<SpecsRepoInfo>, String> {
    openapi::detect_specs_repo(Path::new(&repo_root)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_openapi_bundle(
    repo_root: String,
    entry_file: String,
) -> Result<OpenApiBundleResult, String> {
    let enable_ref_fallback = general_prefs::load_general_prefs()
        .map(|p| p.openapi_ref_fallback_enabled)
        .unwrap_or(true);
    openapi::load_openapi_bundle(Path::new(&repo_root), &entry_file, enable_ref_fallback)
        .map_err(|e| e.to_string())
}
