//! Types for the read-only memory viewer — raw OptMem log rows from
//! project and global stores.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryLogRow {
    pub id: u32,
    pub scope: String,
    pub date: String,
    pub text: String,
    pub store_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryLogPage {
    pub rows: Vec<MemoryLogRow>,
    pub total: u32,
    pub project_store_path: Option<String>,
    pub global_store_path: String,
}

/// Filter for the memory viewer. `scope` is `"project"`, `"global"`, or
/// omitted for both. `search` is a case-insensitive substring match on the
/// raw note text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryLogFilter {
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub repo_root: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub offset: Option<u32>,
}
