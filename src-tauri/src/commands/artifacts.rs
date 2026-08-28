//! Tauri commands for artifacts — thin wrappers over `services::artifacts`.

use crate::domain::artifact::{
    ArtifactContent, ArtifactKind, ArtifactRecord, ArtifactSummary,
};
use crate::domain::artifact_render::RenderedArtifact;
use crate::services::artifacts;

#[tauri::command]
pub fn artifact_list() -> Result<Vec<ArtifactSummary>, String> {
    artifacts::list().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn artifact_get(artifact_id: String) -> Result<ArtifactRecord, String> {
    artifacts::get(&artifact_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn artifact_create_draft(
    kind: ArtifactKind,
    title: String,
    purpose: Option<String>,
    prefill: Option<ArtifactContent>,
    chat_id: Option<String>,
) -> Result<ArtifactRecord, String> {
    artifacts::create_draft(kind, title, purpose, prefill, chat_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn artifact_save(record: ArtifactRecord) -> Result<ArtifactRecord, String> {
    artifacts::save(record).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn artifact_delete(artifact_id: String) -> Result<(), String> {
    artifacts::delete(&artifact_id).map_err(|e| e.to_string())
}

/// Pure projection, no I/O and no stored record involved — the builder
/// calls it on every edit to preview exactly what the assistant will
/// receive, rather than reimplementing `domain::artifact_render` in
/// TypeScript and letting the two drift.
#[tauri::command]
pub fn artifact_render(content: ArtifactContent) -> RenderedArtifact {
    artifacts::render(&content)
}
