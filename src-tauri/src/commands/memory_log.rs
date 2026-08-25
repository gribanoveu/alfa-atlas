//! IPC for the memory viewer UI — query and delete raw OptMem log entries.

use crate::domain::memory_log::{MemoryLogDeleteRequest, MemoryLogFilter, MemoryLogPage};
use crate::services::memory_log;

#[tauri::command]
pub fn memory_log_query(filter: MemoryLogFilter) -> Result<MemoryLogPage, String> {
    memory_log::query(&filter).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn memory_log_delete(request: MemoryLogDeleteRequest) -> Result<(), String> {
    memory_log::delete_entry(&request).map_err(|e| e.to_string())
}
