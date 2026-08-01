use serde::{Deserialize, Serialize};

/// Which part of the filesystem a future AI harness may see for the open
/// project. This is enforced by `services::ai_tools` when it resolves a
/// `ToolScope`'s root — the executor picks the root itself from this mode,
/// it never trusts a caller to already have passed the "right" one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AiAccessMode {
    /// Harness sees only `docsRoot` (the documentation subtree).
    #[default]
    DocsOnly,
    /// Harness sees the entire `repoRoot`, including source code.
    FullRepo,
}
