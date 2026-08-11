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
    Grep,
    GitDiff,
    GitBlame,
    Check,
    WriteFile,
    EditFile,
    DeleteFile,
    CreateDirectory,
    DeleteDirectory,
    Move,
    RequestFullRepoAccess,
    Todo,
    Memory,
    RequestModeSwitch,
    GetAsciidocTemplates,
    AskUser,
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
    ///
    /// `Memory` is *not* in this list even though `note`/`nap`/`forget` do
    /// write to the on-disk OptMem store — its gate depends on which `op`
    /// the call carries (only `note` needs approval), which this
    /// per-`ToolName` bool can't express. See `call_requires_confirmation`,
    /// the actual gate used by `commands::llm`.
    ///
    /// `RequestModeSwitch` is gated the same way as `RequestFullRepoAccess`
    /// — it doesn't mutate anything itself, but it's a model-initiated
    /// change to how the rest of the conversation behaves (system prompt,
    /// tool set), so the user should see it before it takes effect.
    ///
    /// `AskUser` always pauses — it is not a side-effect tool, but the
    /// whole point is to collect a structured answer from the user before
    /// the turn continues (see `commands::llm`'s resume path).
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
                | ToolName::RequestModeSwitch
                | ToolName::AskUser
        )
        // `Todo`/`Grep`/`GitDiff`/`GitBlame`/`Check` are in-memory or
        // read-only, no confirmation gate under any call. `Memory` is
        // handled separately, see the doc comment above.
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
            "grep" => Some(ToolName::Grep),
            "gitDiff" => Some(ToolName::GitDiff),
            "gitBlame" => Some(ToolName::GitBlame),
            "writeFile" => Some(ToolName::WriteFile),
            "editFile" => Some(ToolName::EditFile),
            "deleteFile" => Some(ToolName::DeleteFile),
            "createDirectory" => Some(ToolName::CreateDirectory),
            "deleteDirectory" => Some(ToolName::DeleteDirectory),
            "move" => Some(ToolName::Move),
            "requestFullRepoAccess" => Some(ToolName::RequestFullRepoAccess),
            "todo" => Some(ToolName::Todo),
            "memory" => Some(ToolName::Memory),
            "check" => Some(ToolName::Check),
            "requestModeSwitch" => Some(ToolName::RequestModeSwitch),
            "getAsciidocTemplates" => Some(ToolName::GetAsciidocTemplates),
            "askUser" => Some(ToolName::AskUser),
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
            // Gitignore walk + regex over many files — more than a single
            // file read, still local (no embedding network call).
            ToolName::Grep => 3,
            // Local git2 I/O + unified-diff / blame compaction — a bit more
            // than a bare file read, less than a network embedding search.
            ToolName::GitDiff => 2,
            ToolName::GitBlame => 2,
            // In-memory diagnostics recompute over the workspace index —
            // more than a bare list/read, still local (no network).
            ToolName::Check => 2,
            ToolName::WriteFile => 2,
            ToolName::EditFile => 2,
            ToolName::DeleteFile => 1,
            ToolName::CreateDirectory => 1,
            ToolName::DeleteDirectory => 1,
            ToolName::Move => 2,
            ToolName::RequestFullRepoAccess => 1,
            ToolName::Todo => 1,
            ToolName::Memory => 1,
            ToolName::RequestModeSwitch => 1,
            // Pure in-memory lookup over a fixed static catalog — no I/O,
            // even cheaper than a bare filesystem read.
            ToolName::GetAsciidocTemplates => 1,
            ToolName::AskUser => 1,
        }
    }
}

/// The actual confirmation gate `commands::llm`'s tool-calling loop uses —
/// unlike `ToolName::requires_confirmation`, this can see a call's raw
/// (not yet validated) `arguments` JSON, which `Memory` needs: `op: "note"`
/// writes a new, previously-unreviewed line; `forget` drops TREE summaries;
/// `config` changes store knobs that affect auto-injected context size. Those
/// three pause for approval (or are covered by the user's "always allow" trust
/// for the `memory` tool). `nap` stays auto-executing — it only writes a
/// compression summary the model was explicitly asked to produce earlier in
/// the same turn. Reads (`wake`/`recall`/`zoom`) never pause.
///
/// A call whose `arguments` don't parse is never gated here — `false`, same
/// as an unrecognized `name` — because `services::ai_tools::parse_tool_call`
/// will reject the same malformed JSON before anything can execute, with or
/// without a confirmation pause.
pub fn call_requires_confirmation(name: &str, arguments: &str) -> bool {
    match ToolName::from_wire_name(name) {
        Some(ToolName::Memory) => memory_op_requires_confirmation(arguments),
        Some(tool) => tool.requires_confirmation(),
        None => false,
    }
}

fn memory_op_requires_confirmation(arguments: &str) -> bool {
    #[derive(Deserialize)]
    struct OpFields {
        op: Option<String>,
        knob: Option<String>,
    }
    let Ok(parsed) = serde_json::from_str::<OpFields>(arguments) else {
        return false;
    };
    match parsed.op.as_deref() {
        Some("note" | "forget") => true,
        // Read-only `config` (no knob) just prints sizes — no pause.
        Some("config") => parsed
            .knob
            .as_deref()
            .is_some_and(|k| !k.trim().is_empty()),
        _ => false,
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
        ToolName::Grep,
        ToolName::GitDiff,
        ToolName::GitBlame,
        ToolName::Check,
        ToolName::WriteFile,
        ToolName::EditFile,
        ToolName::DeleteFile,
        ToolName::CreateDirectory,
        ToolName::DeleteDirectory,
        ToolName::Move,
        ToolName::RequestFullRepoAccess,
        ToolName::Todo,
        ToolName::Memory,
        ToolName::RequestModeSwitch,
        ToolName::GetAsciidocTemplates,
        ToolName::AskUser,
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
        assert!(!ToolName::Grep.requires_confirmation());
        assert!(!ToolName::GitDiff.requires_confirmation());
        assert!(!ToolName::GitBlame.requires_confirmation());
        assert!(!ToolName::Check.requires_confirmation());
        assert!(ToolName::WriteFile.requires_confirmation());
        assert!(ToolName::EditFile.requires_confirmation());
        assert!(ToolName::DeleteFile.requires_confirmation());
        assert!(ToolName::CreateDirectory.requires_confirmation());
        assert!(ToolName::DeleteDirectory.requires_confirmation());
        assert!(ToolName::Move.requires_confirmation());
        assert!(ToolName::RequestFullRepoAccess.requires_confirmation());
        assert!(!ToolName::Todo.requires_confirmation());
        assert!(!ToolName::Memory.requires_confirmation());
        assert!(ToolName::RequestModeSwitch.requires_confirmation());
        assert!(!ToolName::GetAsciidocTemplates.requires_confirmation());
        assert!(ToolName::AskUser.requires_confirmation());
    }

    #[test]
    fn from_wire_name_round_trips_every_known_tool() {
        assert_eq!(ToolName::from_wire_name("listFiles"), Some(ToolName::ListFiles));
        assert_eq!(ToolName::from_wire_name("readFile"), Some(ToolName::ReadFile));
        assert_eq!(ToolName::from_wire_name("semanticSearch"), Some(ToolName::SemanticSearch));
        assert_eq!(ToolName::from_wire_name("grep"), Some(ToolName::Grep));
        assert_eq!(ToolName::from_wire_name("gitDiff"), Some(ToolName::GitDiff));
        assert_eq!(ToolName::from_wire_name("gitBlame"), Some(ToolName::GitBlame));
        assert_eq!(ToolName::from_wire_name("check"), Some(ToolName::Check));
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
        assert_eq!(ToolName::from_wire_name("todo"), Some(ToolName::Todo));
        assert_eq!(ToolName::from_wire_name("memory"), Some(ToolName::Memory));
        assert_eq!(
            ToolName::from_wire_name("requestModeSwitch"),
            Some(ToolName::RequestModeSwitch)
        );
        assert_eq!(
            ToolName::from_wire_name("getAsciidocTemplates"),
            Some(ToolName::GetAsciidocTemplates)
        );
        assert_eq!(ToolName::from_wire_name("askUser"), Some(ToolName::AskUser));
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
        assert_eq!(ToolName::Grep.loop_weight(), 3);
        assert_eq!(ToolName::GitDiff.loop_weight(), 2);
        assert_eq!(ToolName::GitBlame.loop_weight(), 2);
        assert_eq!(ToolName::Check.loop_weight(), 2);
        assert_eq!(ToolName::SemanticSearch.loop_weight(), 4);
        assert_eq!(ToolName::Todo.loop_weight(), 1);
        assert_eq!(ToolName::Memory.loop_weight(), 1);
        assert_eq!(ToolName::RequestModeSwitch.loop_weight(), 1);
        assert_eq!(ToolName::GetAsciidocTemplates.loop_weight(), 1);
        assert_eq!(ToolName::AskUser.loop_weight(), 1);
    }

    #[test]
    fn default_allowed_tools_includes_all_nineteen() {
        let allowed = default_allowed_tools(AiAccessMode::DocsOnly);
        assert_eq!(allowed.len(), 19);
        assert!(allowed.contains(&ToolName::Grep));
        assert!(allowed.contains(&ToolName::GitDiff));
        assert!(allowed.contains(&ToolName::GitBlame));
        assert!(allowed.contains(&ToolName::Check));
        assert!(allowed.contains(&ToolName::WriteFile));
        assert!(allowed.contains(&ToolName::EditFile));
        assert!(allowed.contains(&ToolName::DeleteFile));
        assert!(allowed.contains(&ToolName::CreateDirectory));
        assert!(allowed.contains(&ToolName::DeleteDirectory));
        assert!(allowed.contains(&ToolName::Move));
        assert!(allowed.contains(&ToolName::RequestFullRepoAccess));
        assert!(allowed.contains(&ToolName::Todo));
        assert!(allowed.contains(&ToolName::Memory));
        assert!(allowed.contains(&ToolName::RequestModeSwitch));
        assert!(allowed.contains(&ToolName::GetAsciidocTemplates));
        assert!(allowed.contains(&ToolName::AskUser));
    }

    #[test]
    fn call_requires_confirmation_gates_memory_on_mutating_ops() {
        assert!(call_requires_confirmation(
            "memory",
            r#"{"op":"note","scope":"project","text":"a fact"}"#
        ));
        assert!(call_requires_confirmation(
            "memory",
            r#"{"op":"forget","scope":"project","block":"0-1"}"#
        ));
        assert!(call_requires_confirmation(
            "memory",
            r#"{"op":"config","scope":"project","knob":"WAKE_LINES=32"}"#
        ));
        assert!(!call_requires_confirmation(
            "memory",
            r#"{"op":"config","scope":"project"}"#
        ));
        assert!(!call_requires_confirmation(
            "memory",
            r#"{"op":"config","scope":"project","knob":null}"#
        ));
        for op in ["wake", "nap", "recall", "zoom"] {
            let args = format!(r#"{{"op":"{op}","scope":"project"}}"#);
            assert!(
                !call_requires_confirmation("memory", &args),
                "op {op} should not require confirmation"
            );
        }
    }

    #[test]
    fn call_requires_confirmation_fails_closed_on_unparseable_or_missing_op() {
        assert!(!call_requires_confirmation("memory", "not json"));
        assert!(!call_requires_confirmation("memory", r#"{"scope":"project"}"#));
    }

    #[test]
    fn call_requires_confirmation_matches_the_static_check_for_every_other_tool() {
        for tool in [
            ToolName::ListFiles,
            ToolName::ReadFile,
            ToolName::SemanticSearch,
            ToolName::Grep,
            ToolName::GitDiff,
            ToolName::GitBlame,
            ToolName::Check,
            ToolName::WriteFile,
            ToolName::EditFile,
            ToolName::DeleteFile,
            ToolName::CreateDirectory,
            ToolName::DeleteDirectory,
            ToolName::Move,
            ToolName::RequestFullRepoAccess,
            ToolName::Todo,
            ToolName::RequestModeSwitch,
            ToolName::GetAsciidocTemplates,
            ToolName::AskUser,
        ] {
            let name = serde_json::to_value(tool)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();
            assert_eq!(
                call_requires_confirmation(&name, "{}"),
                tool.requires_confirmation(),
                "tool {name} disagreed"
            );
        }
    }

    #[test]
    fn call_requires_confirmation_is_false_for_unknown_tool() {
        assert!(!call_requires_confirmation("notATool", "{}"));
    }
}
