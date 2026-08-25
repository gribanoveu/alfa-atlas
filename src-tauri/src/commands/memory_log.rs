//! Read-only IPC for the memory viewer UI.

use crate::domain::memory_log::{MemoryLogFilter, MemoryLogPage};
use crate::services::memory_log;

#[tauri::command]
pub fn memory_log_query(filter: MemoryLogFilter) -> Result<MemoryLogPage, String> {
    memory_log::query(&filter).map_err(|e| e.to_string())
}
