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

/// Operations a future AI harness may invoke — no longer read-only now that
/// `WriteFile`/`CreateDirectory`/`RequestFullRepoAccess` exist. New variants
/// must also be added to `default_allowed_tools` — nothing is reachable
/// just by existing in this enum, see that function's doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolName {
    ListFiles,
    ReadFile,
    SemanticSearch,
    WriteFile,
    EditFile,
    DeleteFile,
    CreateDirectory,
    DeleteDirectory,
    Move,
    RequestFullRepoAccess,
}

impl ToolName {
    /// Whether any call to this tool must pause the round for user approval
    /// before executing — see `commands::llm`'s tool-calling loop. Static
    /// per tool identity, not per-call: every `WriteFile` call needs
    /// confirmation, regardless of which file it targets. `EditFile`/
    /// `DeleteFile`/`CreateDirectory`/`DeleteDirectory`/`Move` are grouped
    /// with it — same filesystem-mutation family, same "the user should
    /// see this before it happens" reasoning, even though an empty
    /// directory itself carries less risk than overwriting or deleting a
    /// file.
    pub fn requires_confirmation(self) -> bool {
        matches!(
            self,
            ToolName::WriteFile
                | ToolName::EditFile
                | ToolName::DeleteFile
                | ToolName::CreateDirectory
                | ToolName::DeleteDirectory
                | ToolName::Move
                | ToolName::RequestFullRepoAccess
        )
    }

    /// Maps the wire `LlmToolCall::name` string to a `ToolName`, independent
    /// of whether `arguments` parses — the tool-calling loop needs to
    /// classify a call as risky/safe before it's known whether the model's
    /// JSON is even well-formed. Shared with
    /// `services::ai_tools::parse_tool_call`, which is the other place this
    /// exact mapping must hold.
    pub fn from_wire_name(name: &str) -> Option<ToolName> {
        match name {
            "listFiles" => Some(ToolName::ListFiles),
            "readFile" => Some(ToolName::ReadFile),
            "semanticSearch" => Some(ToolName::SemanticSearch),
            "writeFile" => Some(ToolName::WriteFile),
            "editFile" => Some(ToolName::EditFile),
            "deleteFile" => Some(ToolName::DeleteFile),
            "createDirectory" => Some(ToolName::CreateDirectory),
            "deleteDirectory" => Some(ToolName::DeleteDirectory),
            "move" => Some(ToolName::Move),
            "requestFullRepoAccess" => Some(ToolName::RequestFullRepoAccess),
            _ => None,
        }
    }

    /// Relative cost this tool's call charges against `commands::llm`'s
    /// tool-call budget (see `MAX_TOOL_BUDGET`) — an approximation of
    /// time/expense, not risk (risk is `requires_confirmation`'s job).
    /// Local-filesystem-only tools are cheap; `SemanticSearch` is the one
    /// that can hit a network embedding-provider API after cascading
    /// through symbol/lexical matching first (`services::ai_tools::
    /// semantic_search`), so it costs the most. `Move` sits with
    /// `WriteFile`/`EditFile` rather than the bare-syscall tools — besides
    /// the move itself, it can rewrite `include::`/`xref:`/`$ref`
    /// references in other files too (`services::ai_tools::move_path`).
    pub fn loop_weight(self) -> u32 {
        match self {
            ToolName::ListFiles => 1,
            ToolName::ReadFile => 1,
            ToolName::SemanticSearch => 4,
            ToolName::WriteFile => 2,
            ToolName::EditFile => 2,
            ToolName::DeleteFile => 1,
            ToolName::CreateDirectory => 1,
            ToolName::DeleteDirectory => 1,
            ToolName::Move => 2,
            ToolName::RequestFullRepoAccess => 1,
        }
    }
}

/// Tools available when a project hasn't customized its allowlist
/// (`ProjectConfig::ai_allowed_tools == None`). This is only the
/// *fallback* — once a user has set a custom allowlist, it is authoritative
/// and is **not** widened automatically when a new `ToolName` variant is
/// added here later. That's deliberate, fail-closed behavior: a user who
/// narrowed their allowlist has to opt a new tool in explicitly, the same
/// way `AiAccessMode::default()` chooses the safer `DocsOnly` rather than
/// silently granting repo-wide access.
///
/// `WriteFile`/`CreateDirectory`/`RequestFullRepoAccess` are included here
/// too, despite being the tools with real side effects — the per-call
/// confirmation gate (`requires_confirmation`) is the actual safety control
/// for them, not the allowlist. There's no frontend UI to customize this
/// allowlist today, so requiring opt-in here would ship the confirmation
/// feature with no way to ever turn it on.
pub fn default_allowed_tools(_mode: AiAccessMode) -> HashSet<ToolName> {
    [
        ToolName::ListFiles,
        ToolName::ReadFile,
        ToolName::SemanticSearch,
        ToolName::WriteFile,
        ToolName::EditFile,
        ToolName::DeleteFile,
        ToolName::CreateDirectory,
        ToolName::DeleteDirectory,
        ToolName::Move,
        ToolName::RequestFullRepoAccess,
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_confirmation_is_true_only_for_mutating_and_access_escalation_tools() {
        assert!(!ToolName::ListFiles.requires_confirmation());
        assert!(!ToolName::ReadFile.requires_confirmation());
        assert!(!ToolName::SemanticSearch.requires_confirmation());
        assert!(ToolName::WriteFile.requires_confirmation());
        assert!(ToolName::EditFile.requires_confirmation());
        assert!(ToolName::DeleteFile.requires_confirmation());
        assert!(ToolName::CreateDirectory.requires_confirmation());
        assert!(ToolName::DeleteDirectory.requires_confirmation());
        assert!(ToolName::Move.requires_confirmation());
        assert!(ToolName::RequestFullRepoAccess.requires_confirmation());
    }

    #[test]
    fn from_wire_name_round_trips_every_known_tool() {
        assert_eq!(ToolName::from_wire_name("listFiles"), Some(ToolName::ListFiles));
        assert_eq!(ToolName::from_wire_name("readFile"), Some(ToolName::ReadFile));
        assert_eq!(ToolName::from_wire_name("semanticSearch"), Some(ToolName::SemanticSearch));
        assert_eq!(ToolName::from_wire_name("writeFile"), Some(ToolName::WriteFile));
        assert_eq!(ToolName::from_wire_name("editFile"), Some(ToolName::EditFile));
        assert_eq!(ToolName::from_wire_name("deleteFile"), Some(ToolName::DeleteFile));
        assert_eq!(ToolName::from_wire_name("createDirectory"), Some(ToolName::CreateDirectory));
        assert_eq!(ToolName::from_wire_name("deleteDirectory"), Some(ToolName::DeleteDirectory));
        assert_eq!(ToolName::from_wire_name("move"), Some(ToolName::Move));
        assert_eq!(
            ToolName::from_wire_name("requestFullRepoAccess"),
            Some(ToolName::RequestFullRepoAccess)
        );
        assert_eq!(ToolName::from_wire_name("somethingElse"), None);
    }

    #[test]
    fn loop_weight_reflects_relative_cost_per_tool() {
        assert_eq!(ToolName::ListFiles.loop_weight(), 1);
        assert_eq!(ToolName::ReadFile.loop_weight(), 1);
        assert_eq!(ToolName::CreateDirectory.loop_weight(), 1);
        assert_eq!(ToolName::DeleteFile.loop_weight(), 1);
        assert_eq!(ToolName::DeleteDirectory.loop_weight(), 1);
        assert_eq!(ToolName::RequestFullRepoAccess.loop_weight(), 1);
        assert_eq!(ToolName::WriteFile.loop_weight(), 2);
        assert_eq!(ToolName::EditFile.loop_weight(), 2);
        assert_eq!(ToolName::Move.loop_weight(), 2);
        assert_eq!(ToolName::SemanticSearch.loop_weight(), 4);
    }

    #[test]
    fn default_allowed_tools_includes_all_ten() {
        let allowed = default_allowed_tools(AiAccessMode::DocsOnly);
        assert_eq!(allowed.len(), 10);
        assert!(allowed.contains(&ToolName::WriteFile));
        assert!(allowed.contains(&ToolName::EditFile));
        assert!(allowed.contains(&ToolName::DeleteFile));
        assert!(allowed.contains(&ToolName::CreateDirectory));
        assert!(allowed.contains(&ToolName::DeleteDirectory));
        assert!(allowed.contains(&ToolName::Move));
        assert!(allowed.contains(&ToolName::RequestFullRepoAccess));
    }
}
