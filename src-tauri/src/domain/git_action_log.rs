//! Value type for the persisted git action log — what `infra::git_action_log_store`
//! returns and `commands::git_action_log` serializes straight out to the
//! frontend. `payload` is opaque JSON (mirrors `infra::chat_store`'s
//! `messages.data`): its shape is the frontend's `GitActionLogEntry["payload"]`
//! discriminated union, which evolves independently of this struct — Rust's
//! only job is to store and return it byte-for-byte.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitActionLogEntry {
    pub id: String,
    pub kind: String,
    pub summary: String,
    pub payload: serde_json::Value,
    pub undoable: bool,
    pub undone: bool,
    /// Unix milliseconds.
    pub created_at: i64,
}
