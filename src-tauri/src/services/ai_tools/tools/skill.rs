//! `skill` — searching and loading the user's own prompt fragments.

use crate::domain::ai_tools::{SkillArgs, ToolError, ToolResult};
use crate::domain::llm::LlmToolDefinition;

pub(super) fn execute_skill(args: SkillArgs) -> Result<ToolResult, ToolError> {
    match args.op.as_str() {
        "search" => {
            let query = args.query.as_deref().unwrap_or("");
            crate::services::agent_skills::search(query).map_err(ToolError::from)
        }
        "load" => {
            let name = args.name.as_deref().unwrap_or("");
            if name.is_empty() {
                return Err(ToolError::InvalidArguments {
                    tool: "skill".to_string(),
                    reason: "load requires `name`".to_string(),
                });
            }
            crate::services::agent_skills::load(name).map_err(ToolError::from)
        }
        "read" => {
            let name = args.name.as_deref().unwrap_or("");
            let path = args.path.as_deref().unwrap_or("");
            if name.is_empty() || path.is_empty() {
                return Err(ToolError::InvalidArguments {
                    tool: "skill".to_string(),
                    reason: "read requires `name` and `path`".to_string(),
                });
            }
            crate::services::agent_skills::read(name, path).map_err(ToolError::from)
        }
        other => Err(ToolError::from(
            crate::domain::agent_skills::SkillError::UnknownOp(other.to_string()),
        )),
    }
}

/// The `skill` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "skill".to_string(),
        description:
            "Search and load specialized instruction packs (skills) on demand. Do not guess skill names and do not expect a catalog in this description. First call with op \"search\" and a short query about the current task (required — empty query is rejected). Then op \"load\" with a matching name to get full instructions. If those instructions point to a companion file, op \"read\" with name and path. Use this before writing or filling REST/Thrift method documentation, or when working with OpenAPI specs layout (schemas/operations/$ref) or any user-installed pack. Ordinary AsciiDoc authoring does not need a skill — do not search for one just because the request mentions documentation in general."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "op": {
                    "type": "string",
                    "enum": ["search", "load", "read"],
                    "description": "search: find skills by query. load: full SKILL.md body. read: one companion file."
                },
                "query": {
                    "type": ["string", "null"],
                    "description": "Required for op search. Short description of the task (not empty)."
                },
                "name": {
                    "type": ["string", "null"],
                    "description": "Skill name from a search hit. Required for load and read."
                },
                "path": {
                    "type": ["string", "null"],
                    "description": "Companion file path relative to the skill root. Required for read."
                }
            },
            "required": ["op"]
        }),
        }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::domain::ai_access::AiAccessMode;
    use crate::domain::ai_tools::{SkillArgs, ToolCall, ToolError, ToolResult, ToolScope};
    use crate::services::ai_tools::testing::*;
    use crate::services::ai_tools::{EmbeddingDeps, execute_tool};

    #[test]
    fn execute_tool_skill_search_does_not_return_disabled() {
        crate::infra::settings_store::test_support::with_temp_home(|| {
            crate::services::agent_skills::set_skill_enabled(
                crate::domain::agent_skills::SkillSource::Bundled,
                "method-spec",
                false,
            )
            .unwrap();
            let (repo, docs) = fixture_repo();
            let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
            let result = execute_tool(
                &scope,
                ToolCall::Skill(SkillArgs {
                    op: "search".to_string(),
                    query: Some("REST method folder documentation".to_string()),
                    name: None,
                    path: None,
                }),
                &EmbeddingDeps::empty(),
                &[],
            )
            .unwrap();
            match result {
                ToolResult::SkillSearch(hits) => {
                    assert!(!hits.matches.iter().any(|m| m.name == "method-spec"));
                }
                other => panic!("expected SkillSearch, got {other:?}"),
            }
            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn execute_tool_skill_load_unknown_name_errors() {
        crate::infra::settings_store::test_support::with_temp_home(|| {
            let (repo, docs) = fixture_repo();
            let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
            let err = execute_tool(
                &scope,
                ToolCall::Skill(SkillArgs {
                    op: "load".to_string(),
                    query: None,
                    name: Some("no-such-skill".to_string()),
                    path: None,
                }),
                &EmbeddingDeps::empty(),
                &[],
            )
            .unwrap_err();
            assert!(matches!(err, ToolError::NotFound(_)));
            fs::remove_dir_all(&repo).ok();
        });
    }
}
