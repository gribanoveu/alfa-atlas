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
    /// Read-only research + a structured plan via `createPlan` — no
    /// mutation tools are even offered, so the model cannot attempt to
    /// execute a plan it just proposed (see `assistantConfig.ts`'s
    /// `buildPlanModeSystemPrompt`).
    Plan,
    /// Lightest mode — direct, read-only Q&A with no planning ceremony.
    Question,
}

/// Tools offered in every mode — pure read/inspection tools plus
/// `RequestModeSwitch` / `AskUser`, which must always be reachable
/// regardless of which mode the model is currently in (`AskUser` is the
/// mid-turn clarifying-question pause; see `domain::ai_tools::AskUserArgs`).
pub fn base_tools() -> HashSet<ToolName> {
    [
        ToolName::ListFiles,
        ToolName::ReadFile,
        ToolName::SemanticSearch,
        ToolName::Grep,
        ToolName::GitDiff,
        ToolName::GitBlame,
        ToolName::Check,
        ToolName::RequestModeSwitch,
        // Read-only lookup over a fixed catalog — useful in every mode:
        // Agent to actually draft with it, Plan to reference the exact
        // shape while planning a future edit, Question to answer "how do we
        // format X" without needing write access.
        ToolName::GetAsciidocTemplates,
        ToolName::AskUser,
        // Reading an artifact the user filled in earlier is an ordinary
        // read — useful in Agent to write from it, Plan to ground a plan in
        // it, Question to answer "what does this endpoint take" from it.
        // *Requesting* a new one is not in this set: see
        // `extra_tools_for_mode`.
        ToolName::Artifact,
        ToolName::Skill,
        // Drawing a diagram of code the model just read is display-only —
        // no writes, no access change — and answering "как это работает"
        // with a picture is most valuable in Question mode, the one mode
        // that gets no extras at all. So it belongs here, not in
        // `extra_tools_for_mode`.
        ToolName::Visualize,
    ]
    .into_iter()
    .collect()
}

/// Tools added on top of `base_tools` for one specific mode. `Plan` gets
/// `RequestFullRepoAccess` (a wider *read* boundary helps ground a plan in
/// real code, not just docs), plus `CreatePlan`/`UpdatePlan`/`ReadPlan` for
/// the structured plan artifact — deliberately not `Todo` (the chat
/// working-memory checklist) or `UpdatePlanTodo` (execution tracking, Agent
/// only). `Question` gets nothing extra — the leanest tool set, for
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
            ToolName::ReadPlan,
            ToolName::UpdatePlanTodo,
            ToolName::RequestArtifact,
        ]
        .into_iter()
        .collect(),
        ConversationMode::Plan => [
            ToolName::RequestFullRepoAccess,
            ToolName::CreatePlan,
            ToolName::UpdatePlan,
            ToolName::ReadPlan,
            // Plan mode drafts the document's shape, so it is a legitimate
            // place to discover that the request/response facts are missing
            // and ask for them before planning around a guess.
            ToolName::RequestArtifact,
        ]
        .into_iter()
        .collect(),
        // Deliberately no `RequestArtifact`: Question mode answers from
        // what exists. Popping a form builder in front of someone who asked
        // a quick question is the opposite of this mode's point — the model
        // should say what it doesn't know and let the user switch modes.
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
        assert_eq!(mode_tools(ConversationMode::Agent).len(), 24);
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
            ToolName::UpdatePlanTodo,
        ] {
            assert!(!tools.contains(&mutating), "{mutating:?} should not be in Plan mode");
        }
        assert!(tools.contains(&ToolName::RequestFullRepoAccess));
        assert!(tools.contains(&ToolName::RequestModeSwitch));
        assert!(tools.contains(&ToolName::AskUser));
        assert!(tools.contains(&ToolName::Skill));
        assert!(tools.contains(&ToolName::CreatePlan));
        assert!(tools.contains(&ToolName::UpdatePlan));
        assert!(tools.contains(&ToolName::ReadPlan));
        assert!(tools.contains(&ToolName::RequestArtifact));
        assert!(tools.contains(&ToolName::Artifact));
    }

    #[test]
    fn question_mode_can_read_an_artifact_but_not_request_one() {
        let tools = mode_tools(ConversationMode::Question);
        assert!(tools.contains(&ToolName::Artifact));
        assert!(!tools.contains(&ToolName::RequestArtifact));
    }

    #[test]
    fn visualize_is_reachable_from_every_mode() {
        // A diagram is display-only, and "объясни, как это работает" is a
        // Question-mode question as often as an Agent one.
        for mode in [ConversationMode::Agent, ConversationMode::Plan, ConversationMode::Question] {
            assert!(mode_tools(mode).contains(&ToolName::Visualize));
        }
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

    #[test]
    fn ask_user_is_reachable_from_every_mode() {
        for mode in [ConversationMode::Agent, ConversationMode::Plan, ConversationMode::Question] {
            assert!(mode_tools(mode).contains(&ToolName::AskUser));
        }
    }

    #[test]
    fn skill_is_reachable_from_every_mode() {
        for mode in [ConversationMode::Agent, ConversationMode::Plan, ConversationMode::Question] {
            assert!(mode_tools(mode).contains(&ToolName::Skill));
        }
    }

    #[test]
    fn memory_is_not_an_agent_tool() {
        for mode in [ConversationMode::Agent, ConversationMode::Plan, ConversationMode::Question] {
            assert!(!mode_tools(mode).contains(&ToolName::Memory));
        }
    }
}
