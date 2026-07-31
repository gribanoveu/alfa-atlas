use std::path::PathBuf;

use crate::domain::standards::{RuleDef, StandardsReport, StandardsRuleConfig};
use crate::services::{standards, standards_prefs, standards_rules};

#[tauri::command]
pub fn get_standards_rules() -> Vec<RuleDef> {
    standards_rules::RULES.iter().map(|r| r.def).collect()
}

#[tauri::command]
pub fn get_standards_config() -> Result<StandardsRuleConfig, String> {
    standards_prefs::load_standards_config().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_standards_config(config: StandardsRuleConfig) -> Result<(), String> {
    standards_prefs::save_standards_config(config).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_standards(docs_root: String) -> Result<StandardsReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = standards_prefs::load_standards_config().unwrap_or_default();
        standards::check_repository(&PathBuf::from(docs_root), &config)
    })
    .await
    .map_err(|e| e.to_string())
}
