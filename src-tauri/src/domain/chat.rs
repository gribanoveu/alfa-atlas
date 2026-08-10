//! The cross-boundary value types for persisted assistant chat history —
//! what `infra::chat_store` returns and `commands::chat_history` serializes
//! straight out to the frontend. Message content itself never gets a
//! Rust-side type (see `infra::chat_store`'s module doc) — only the summary
//! row shape and the todo checklist (already a stable, shared domain type)
//! are typed here.

use serde::Serialize;

use super::ai_tools::Task;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSummary {
    pub id: String,
    pub repo_root: String,
    pub title: String,
    pub archived: bool,
    /// Unix milliseconds.
    pub created_at: i64,
    /// Unix milliseconds.
    pub updated_at: i64,
}

/// One chat's full persisted state — messages (opaque JSON, see
/// `infra::chat_store`'s module doc) plus its typed todo checklist.
/// Combined into one round-trip type rather than a separate "load todos"
/// call: every caller (`useChatHistory`'s mount effect, `switchChat`)
/// always needs both together.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedChat {
    pub messages: Vec<serde_json::Value>,
    pub todos: Vec<Task>,
}
