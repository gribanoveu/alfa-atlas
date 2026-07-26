//! Tauri commands for the async AsciiDoc parse flow.
//!
//! The frontend calls `submit_asciidoc_facts` to deliver parsed facts back
//! to the index, and `frontend_ready` once the listener is mounted so the
//! Rust coordinator can drain any buffered parse requests.

use std::sync::Arc;

use tauri::State;

use crate::domain::asciidoc_facts::AsciiDocFacts;
use crate::domain::workspace_index::DocumentId;
use crate::services::workspace_index::WorkspaceIndex;

#[tauri::command]
pub fn submit_asciidoc_facts(
    index: State<'_, Arc<WorkspaceIndex>>,
    document_id: String,
    version: u64,
    facts: AsciiDocFacts,
) -> Result<(), String> {
    let doc_id = DocumentId::new(document_id);
    index
        .submit_asciidoc_facts(&doc_id, version, facts)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn frontend_ready(index: State<'_, Arc<WorkspaceIndex>>) -> Result<(), String> {
    index.frontend_ready();
    Ok(())
}
