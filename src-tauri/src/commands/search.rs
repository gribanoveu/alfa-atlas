use crate::domain::ai_tools::GrepArgs;
use crate::domain::docs_search::GrepResultsPayload;
use crate::services::docs_search as docs_search_svc;

/// Exact regex content search under the project's documentation root —
/// user-facing counterpart to the AI `grep` tool. Always DocsOnly: never
/// consults `AiAccessMode` / `ai_allowed_tools`.
#[tauri::command]
pub fn docs_search(docs_root: String, args: GrepArgs) -> Result<GrepResultsPayload, String> {
    docs_search_svc::search_docs(&docs_root, &args).map_err(|e| e.to_string())
}
