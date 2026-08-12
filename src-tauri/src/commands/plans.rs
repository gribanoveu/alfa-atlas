//! Tauri commands for persisted work plans — thin wrappers over
//! `services::plans`.

use crate::domain::plan::{PlanRecord, PlanSummary};
use crate::services::plans;

#[tauri::command]
pub fn plan_list() -> Result<Vec<PlanSummary>, String> {
    plans::list_plans().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plan_get(plan_id: String) -> Result<PlanRecord, String> {
    plans::get_plan(&plan_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plan_delete(plan_id: String) -> Result<(), String> {
    plans::delete_plan(&plan_id).map_err(|e| e.to_string())
}
