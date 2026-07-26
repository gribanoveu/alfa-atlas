use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::domain::workspace_index::{
    Anchor, Attribute, Diagnostic, Document, Image, Include, IndexStats, Reference,
};
use crate::services::workspace_index::WorkspaceIndex;

#[tauri::command]
pub async fn build_index(
    index: State<'_, Arc<WorkspaceIndex>>,
    repo_root: String,
) -> Result<IndexStats, String> {
    let index = index.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        index
            .build(PathBuf::from(repo_root))
            .and_then(|stats| {
                index.start_watcher()?;
                Ok(stats)
            })
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn clear_index(index: State<'_, Arc<WorkspaceIndex>>) -> Result<(), String> {
    index.clear();
    Ok(())
}

#[tauri::command]
pub fn index_is_open(index: State<'_, Arc<WorkspaceIndex>>) -> bool {
    index.is_open()
}

#[tauri::command]
pub fn get_document(
    index: State<'_, Arc<WorkspaceIndex>>,
    path: String,
) -> Result<Option<Document>, String> {
    Ok(index.get_document(std::path::Path::new(&path)))
}

#[tauri::command]
pub fn get_documents(index: State<'_, Arc<WorkspaceIndex>>) -> Result<Vec<Document>, String> {
    Ok(index.get_documents())
}

#[tauri::command]
pub fn find_document(
    index: State<'_, Arc<WorkspaceIndex>>,
    name: String,
) -> Result<Vec<Document>, String> {
    Ok(index.find_document(&name))
}

#[tauri::command]
pub fn find_anchor(
    index: State<'_, Arc<WorkspaceIndex>>,
    id: String,
) -> Result<Vec<Anchor>, String> {
    Ok(index.find_anchor(&id))
}

#[tauri::command]
pub fn find_anchors(
    index: State<'_, Arc<WorkspaceIndex>>,
    document_id: String,
) -> Result<Vec<Anchor>, String> {
    Ok(index.find_anchors(&crate::domain::workspace_index::DocumentId::new(
        document_id,
    )))
}

#[tauri::command]
pub fn find_includes(
    index: State<'_, Arc<WorkspaceIndex>>,
    document_id: String,
) -> Result<Vec<Include>, String> {
    Ok(index.find_includes(&crate::domain::workspace_index::DocumentId::new(
        document_id,
    )))
}

#[tauri::command]
pub fn find_references(
    index: State<'_, Arc<WorkspaceIndex>>,
    document_id: String,
) -> Result<Vec<Reference>, String> {
    Ok(index.find_references(&crate::domain::workspace_index::DocumentId::new(
        document_id,
    )))
}

#[tauri::command]
pub fn find_attribute(
    index: State<'_, Arc<WorkspaceIndex>>,
    name: String,
) -> Result<Vec<Attribute>, String> {
    Ok(index.find_attribute(&name))
}

#[tauri::command]
pub fn get_attributes(
    index: State<'_, Arc<WorkspaceIndex>>,
    document_id: String,
) -> Result<Vec<Attribute>, String> {
    Ok(index.get_attributes(&crate::domain::workspace_index::DocumentId::new(
        document_id,
    )))
}

#[tauri::command]
pub fn find_image(
    index: State<'_, Arc<WorkspaceIndex>>,
    path: String,
) -> Result<Vec<Image>, String> {
    Ok(index.find_image(&path))
}

#[tauri::command]
pub fn get_diagnostics(
    index: State<'_, Arc<WorkspaceIndex>>,
) -> Result<Vec<Diagnostic>, String> {
    Ok(index.get_diagnostics())
}

#[tauri::command]
pub fn get_diagnostics_for(
    index: State<'_, Arc<WorkspaceIndex>>,
    document_id: String,
) -> Result<Vec<Diagnostic>, String> {
    Ok(index.get_diagnostics_for(&crate::domain::workspace_index::DocumentId::new(
        document_id,
    )))
}
