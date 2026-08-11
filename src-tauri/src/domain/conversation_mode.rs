use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::ai_access::ToolName;

/// Which behavioral mode the current chat turn is in — distinct from
/// `AiAccessMode` (a filesystem-boundary/security concept, persisted per
/// project) and from `ToolScope`'s project-level `ai_allowed_tools`
/// allowlist. This is a per-chat-session setting the frontend threads in
/// explicitly on every turn (see `commands::llm::llm_chat_stream`), never
/// persisted to `ProjectConfig` — it composes with, rather than replaces,
/// those other two axes: a tool must be allowed by the project *and* by the
/// current mode to actually reach the model, see `mode_tools`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConversationMode {
    /// Full harness: every tool, including all filesystem mutations.
    Agent,
    /// Read-only research + a structured plan as the text response — no
    /// mutation tools are even offered, so the model cannot attempt to
    /// execute a plan it just proposed (see `assistantConfig.ts`'s
    /// `buildPlanModeSystemPrompt`).
    Plan,
    /// Lightest mode — direct, read-only Q&A with no planning ceremony.
    Question,
}

/// Tools offered in every mode — pure read/inspection tools plus
/// `RequestModeSwitch` itself, which must always be reachable regardless of
/// which mode the model is currently in.
pub fn base_tools() -> HashSet<ToolName> {
    [
        ToolName::ListFiles,
        ToolName::ReadFile,
        ToolName::SemanticSearch,
        ToolName::Grep,
        ToolName::GitDiff,
        ToolName::GitBlame,
        ToolName::Check,
        ToolName::Memory,
        ToolName::RequestModeSwitch,
        // Read-only lookup over a fixed catalog — useful in every mode:
        // Agent to actually draft with it, Plan to reference the exact
        // shape while planning a future edit, Question to answer "how do we
        // format X" without needing write access.
        ToolName::GetAsciidocTemplates,
    ]
    .into_iter()
    .collect()
}

/// Tools added on top of `base_tools` for one specific mode. `Plan` gets
/// `RequestFullRepoAccess` (a wider *read* boundary helps ground a plan in
/// real code, not just docs) but deliberately not `Todo` — a plan is
/// delivered as the turn's text response, not a todo checklist; `Todo` is a
/// working-memory aid for actually executing a task, which only `Agent`
/// does. `Question` gets nothing extra — the leanest tool set, for
/// lightweight point answers.
pub fn extra_tools_for_mode(mode: ConversationMode) -> HashSet<ToolName> {
    match mode {
        ConversationMode::Agent => [
            ToolName::WriteFile,
            ToolName::EditFile,
            ToolName::DeleteFile,
            ToolName::CreateDirectory,
            ToolName::DeleteDirectory,
            ToolName::Move,
            ToolName::RequestFullRepoAccess,
            ToolName::Todo,
        ]
        .into_iter()
        .collect(),
        ConversationMode::Plan => [ToolName::RequestFullRepoAccess].into_iter().collect(),
        ConversationMode::Question => HashSet::new(),
    }
}

/// The full set of tools reachable in `mode` — `base_tools() ∪
/// extra_tools_for_mode(mode)`. `services::ai_tools::llm_tool_definitions`
/// intersects this with the project's own `ToolScope::allowed_tools` when
/// deciding what to actually advertise to the model; `commands::llm::
/// run_tool_loop` re-checks it before executing any call, as defense in
/// depth against a hallucinated or mode-stale tool name.
pub fn mode_tools(mode: ConversationMode) -> HashSet<ToolName> {
    base_tools().union(&extra_tools_for_mode(mode)).copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_mode_has_every_tool() {
        assert_eq!(mode_tools(ConversationMode::Agent).len(), 18);
    }

    #[test]
    fn plan_mode_excludes_every_mutation_tool_and_todo() {
        let tools = mode_tools(ConversationMode::Plan);
        for mutating in [
            ToolName::WriteFile,
            ToolName::EditFile,
            ToolName::DeleteFile,
            ToolName::CreateDirectory,
            ToolName::DeleteDirectory,
            ToolName::Move,
            ToolName::Todo,
        ] {
            assert!(!tools.contains(&mutating), "{mutating:?} should not be in Plan mode");
        }
        assert!(tools.contains(&ToolName::RequestFullRepoAccess));
        assert!(tools.contains(&ToolName::RequestModeSwitch));
    }

    #[test]
    fn question_mode_is_exactly_the_base_set() {
        assert_eq!(mode_tools(ConversationMode::Question), base_tools());
    }

    #[test]
    fn request_mode_switch_is_reachable_from_every_mode() {
        for mode in [ConversationMode::Agent, ConversationMode::Plan, ConversationMode::Question] {
            assert!(mode_tools(mode).contains(&ToolName::RequestModeSwitch));
        }
    }
}
