use std::collections::HashSet;

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

/// Read-only operations a future AI harness may invoke. New variants must
/// also be added to `default_allowed_tools` — nothing is reachable just by
/// existing in this enum, see that function's doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolName {
    ListFiles,
    ReadFile,
    SemanticSearch,
}

/// Tools available when a project hasn't customized its allowlist
/// (`ProjectConfig::ai_allowed_tools == None`). This is only the
/// *fallback* — once a user has set a custom allowlist, it is authoritative
/// and is **not** widened automatically when a new `ToolName` variant is
/// added here later. That's deliberate, fail-closed behavior: a user who
/// narrowed their allowlist has to opt a new tool in explicitly, the same
/// way `AiAccessMode::default()` chooses the safer `DocsOnly` rather than
/// silently granting repo-wide access.
pub fn default_allowed_tools(_mode: AiAccessMode) -> HashSet<ToolName> {
    [ToolName::ListFiles, ToolName::ReadFile, ToolName::SemanticSearch]
        .into_iter()
        .collect()
}
