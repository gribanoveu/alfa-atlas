use crate::domain::settings::GeneralPrefs;
use crate::infra::settings_store;
use crate::services::general_prefs;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPaths {
    pub user_settings_dir: String,
    pub plans_dir: String,
    pub artifacts_dir: String,
    pub project_root: Option<String>,
    pub project_config_dir: Option<String>,
}

#[tauri::command]
pub fn get_general_prefs() -> Result<GeneralPrefs, String> {
    general_prefs::load_general_prefs().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_general_prefs(prefs: GeneralPrefs) -> Result<(), String> {
    general_prefs::save_general_prefs(prefs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings_paths() -> Result<SettingsPaths, String> {
    let user_settings_dir = settings_store::settings_dir()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();
    let plans_dir = crate::services::plans::plans_root_path()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();
    let artifacts_dir = crate::services::artifacts::artifacts_root_path()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();

    let settings = settings_store::load().map_err(|e| e.to_string())?;
    let project_root = settings.project.root.clone();
    let project_config_dir = project_root
        .as_ref()
        .map(|root| std::path::Path::new(root).join(".atlas").to_string_lossy().into_owned());

    Ok(SettingsPaths {
        user_settings_dir,
        plans_dir,
        artifacts_dir,
        project_root,
        project_config_dir,
    })
}
