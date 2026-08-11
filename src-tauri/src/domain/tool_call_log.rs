//! Value types for the persisted tool-call log — what
//! `infra::tool_call_log` returns and `commands::tool_call_log` serializes
//! straight out to the frontend. `args_json`/`result_json` are already
//! redacted by the time they reach this struct (see
//! `infra::tool_call_log::redact_args`/`redact_result`) — this module never
//! sees raw document content, only the structural/identifying remainder.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallLogRow {
    pub id: i64,
    /// Unix milliseconds.
    pub ts_ms: i64,
    pub repo_root: String,
    /// `"chat"` (the LLM tool-calling loop) or `"standalone"` (the
    /// `ai_execute_tool` IPC command, which has no chat turn around it).
    pub source: String,
    /// Tool-calling round within a chat turn — `None` for `"standalone"`.
    pub round: Option<u32>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub tool: String,
    pub args_json: serde_json::Value,
    /// `"ok"` or `"error"`.
    pub status: String,
    pub error_message: Option<String>,
    /// `None` when `status` is `"error"`.
    pub result_json: Option<serde_json::Value>,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallLogPage {
    pub rows: Vec<ToolCallLogRow>,
    /// Total rows matching the filter, ignoring `limit`/`offset` — for
    /// pagination controls.
    pub total: i64,
}

/// All fields optional/absent = no filtering, newest first, default page
/// size (see `infra::tool_call_log::query`'s clamping of `limit`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallLogFilter {
    pub repo_root: Option<String>,
    pub tool: Option<String>,
    /// `"ok"` or `"error"`.
    pub status: Option<String>,
    /// Case-insensitive substring match against `tool` and `error_message`.
    pub search: Option<String>,
    /// Unix milliseconds — only rows at or after this timestamp.
    pub since_ms: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
