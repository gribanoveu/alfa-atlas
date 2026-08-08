//! The one cross-boundary value type for persisted assistant chat history —
//! what `infra::chat_store` returns and `commands::chat_history` serializes
//! straight out to the frontend. Message content itself never gets a
//! Rust-side type (see `infra::chat_store`'s module doc) — only this
//! summary row shape is shared.

use serde::Serialize;

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
