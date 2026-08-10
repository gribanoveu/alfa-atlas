//! Tauri command for exporting user-generated content to an arbitrary,
//! user-chosen filesystem path — distinct from `commands::project`'s
//! docs_fs-backed writes, which are deliberately scoped/canonicalized under
//! a project's docs_root. The path here always comes from the native save
//! dialog (`@tauri-apps/plugin-dialog`'s `save()`), so no scoping/extension
//! allowlist is needed the way `docs_fs::write_project_file` has one.

#[tauri::command]
pub fn write_export_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| e.to_string())
}
