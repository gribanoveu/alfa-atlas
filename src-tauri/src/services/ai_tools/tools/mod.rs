//! One module per tool, each holding both the JSON schema the model sees
//! and the implementation behind it. Keeping the pair in one file is the
//! point of this layout: a field added to `WriteFileArgs` without a
//! matching schema entry is a bug the model can never report, and the two
//! used to sit 1500 lines apart.
//!
//! This module is the only place either half fans out: `DEFINITIONS` for
//! what the model is told exists, `execute_tool` for what actually runs.

mod asciidoc_templates;
mod check;
mod conversation;
mod create_directory;
mod delete_directory;
mod delete_file;
mod edit_file;
mod git;
mod grep;
mod list_files;
mod move_path;
mod plans;
mod read_file;
mod semantic_search;
mod skill;
mod todo;
mod write_file;

use crate::domain::ai_access::{AiAccessMode, ToolName};
use crate::domain::ai_tools::{Task, ToolCall, ToolError, ToolResult, ToolScope};
use crate::domain::conversation_mode::{ConversationMode, mode_tools};
use crate::domain::llm::LlmToolDefinition;

pub use list_files::render_file_tree;

use super::scope::set_access_mode;
use super::EmbeddingDeps;

/// One row of `DEFINITIONS`: a tool's name plus the function that builds
/// its schema on demand.
type ToolDefinitionRow = (ToolName, fn() -> LlmToolDefinition);

/// Every tool the harness knows how to run, paired with its schema, in the
/// order the model is shown them. Adding a tool means adding a module and
/// one row here — nothing else in this file changes.
const DEFINITIONS: &[ToolDefinitionRow] = &[
    (ToolName::ListFiles, list_files::definition),
    (ToolName::ReadFile, read_file::definition),
    (ToolName::SemanticSearch, semantic_search::definition),
    (ToolName::Grep, grep::definition),
    (ToolName::GitDiff, git::diff_definition),
    (ToolName::GitBlame, git::blame_definition),
    (ToolName::Check, check::definition),
    (ToolName::WriteFile, write_file::definition),
    (ToolName::EditFile, edit_file::definition),
    (ToolName::DeleteFile, delete_file::definition),
    (ToolName::CreateDirectory, create_directory::definition),
    (ToolName::DeleteDirectory, delete_directory::definition),
    (ToolName::Move, move_path::definition),
    (ToolName::RequestFullRepoAccess, conversation::full_repo_access_definition),
    (ToolName::Todo, todo::definition),
    (ToolName::RequestModeSwitch, conversation::mode_switch_definition),
    (ToolName::GetAsciidocTemplates, asciidoc_templates::definition),
    (ToolName::Skill, skill::definition),
    (ToolName::AskUser, conversation::ask_user_definition),
    (ToolName::CreatePlan, plans::create_definition),
    (ToolName::UpdatePlan, plans::update_definition),
    (ToolName::ReadPlan, plans::read_definition),
    (ToolName::UpdatePlanTodo, plans::update_todo_definition),
];

/// One `LlmToolDefinition` per tool `scope` allows, to advertise to the
/// model — so a customized (narrowed) allowlist only offers tools that will
/// actually succeed if called, rather than the model discovering
/// `ToolError::NotAllowed` only at execution time. Wire tag values and
/// argument field names are hand-kept in sync with the `ToolCall` variants
/// — see the schema round-trip test, which catches drift between the two.
pub fn llm_tool_definitions(
    scope: &ToolScope,
    conversation_mode: ConversationMode,
) -> Vec<LlmToolDefinition> {
    // A tool reaches the model only if it clears *both* independent axes:
    // the project's own allowlist (`scope`, persisted, "does this project
    // permit this tool at all") and the current conversation mode
    // (`mode_tools`, per-session, "does this task-type need it right now").
    let allowed_now = mode_tools(conversation_mode);
    DEFINITIONS
        .iter()
        .filter(|(tool, _)| scope.allows(*tool) && allowed_now.contains(tool))
        .map(|(_, build)| build())
        .collect()
}

/// Runs one parsed call. The allowlist check happens here rather than in
/// each tool, so a narrowed project allowlist is enforced even for a call
/// that reached us some other way than through `llm_tool_definitions`.
pub fn execute_tool(
    scope: &ToolScope,
    call: ToolCall,
    deps: &EmbeddingDeps,
    todos: &[Task],
) -> Result<ToolResult, ToolError> {
    if !scope.allows(call.name()) {
        return Err(ToolError::NotAllowed(call.name()));
    }
    match call {
        ToolCall::ReadFile(args) => {
            read_file::read_file(scope, args).map(|slice| ToolResult::File {
                content: slice.content,
                start_line: slice.start_line,
                end_line: slice.end_line,
                total_lines: slice.total_lines,
            })
        }
        ToolCall::ListFiles(args) => {
            list_files::list_files(scope, args).map(ToolResult::FileList)
        }
        ToolCall::SemanticSearch(args) => semantic_search::semantic_search(scope, args, deps)
            .map(ToolResult::SemanticSearchResults),
        ToolCall::Grep(args) => grep::grep(scope, args),
        ToolCall::GitDiff(args) => git::git_diff(scope, args),
        ToolCall::GitBlame(args) => git::git_blame(scope, args),
        ToolCall::Check(args) => check::check(scope, args, deps),
        ToolCall::WriteFile(args) => write_file::write_file(scope, args, deps)
            .map(|(path, diff)| ToolResult::FileWritten { path, diff }),
        ToolCall::EditFile(args) => {
            edit_file::edit_file(scope, args, deps.fast_apply.as_ref(), deps)
                .map(|(path, diff)| ToolResult::FileEdited { path, diff })
        }
        ToolCall::DeleteFile(args) => delete_file::delete_file(scope, args, deps)
            .map(|(path, diff)| ToolResult::FileDeleted { path, diff }),
        ToolCall::CreateDirectory(args) => create_directory::create_directory(scope, args, deps)
            .map(|(path, template, created_files)| ToolResult::DirectoryCreated {
                path,
                template,
                created_files,
            }),
        ToolCall::DeleteDirectory(args) => delete_directory::delete_directory(scope, args, deps)
            .map(|path| ToolResult::DirectoryDeleted { path }),
        ToolCall::Move(args) => move_path::move_path(scope, args, deps).map(
            |(from, to, updated_files)| ToolResult::Moved { from, to, updated_files },
        ),
        ToolCall::RequestFullRepoAccess(_args) => set_access_mode(AiAccessMode::FullRepo)
            .map(|()| ToolResult::AccessModeChanged { mode: AiAccessMode::FullRepo })
            .map_err(ToolError::from),
        ToolCall::TodoWrite(args) => todo::todo_write(todos, args).map(ToolResult::TodoWritten),
        ToolCall::TodoUpdate(args) => todo::todo_update(todos, args).map(ToolResult::TodoUpdated),
        ToolCall::Memory(_) => Err(ToolError::InvalidArguments {
            tool: "memory".to_string(),
            reason: "the memory tool was removed; long-term memory is managed automatically by the harness".to_string(),
        }),
        // No state to mutate — `ConversationMode` isn't persisted anywhere
        // server-side (see `domain::conversation_mode`'s doc comment); this
        // is a pure acknowledgement the frontend reacts to once the call
        // settles, same as `services::llm_chat::run_tool_loop` deliberately
        // does *not* re-scope mid-round for this tool the way it does for
        // `RequestFullRepoAccess`.
        ToolCall::RequestModeSwitch(args) => {
            Ok(ToolResult::ModeSwitchRequested { mode: args.mode, reason: args.reason })
        }
        ToolCall::GetAsciidocTemplates(args) => {
            Ok(asciidoc_templates::get_asciidoc_templates(args))
        }
        ToolCall::Skill(args) => skill::execute_skill(args),
        // Never produced here — answers come from `ToolCallDecision::answer`
        // on `llm_chat_stream_resume`. Calling via bare `ai_execute_tool`
        // without a user answer is a programming error, not a model recovery
        // case (the model never sees this path for a well-formed pause).
        ToolCall::AskUser(_) => Err(ToolError::InvalidArguments {
            tool: "askUser".to_string(),
            reason: "askUser must be answered via resume, not execute_tool".to_string(),
        }),
        ToolCall::CreatePlan(args) => plans::create_plan(args),
        ToolCall::UpdatePlan(args) => plans::update_plan(args),
        ToolCall::ReadPlan(args) => plans::read_plan(args),
        ToolCall::UpdatePlanTodo(args) => plans::update_plan_todo(args),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use std::collections::HashSet;

    use crate::domain::ai_access::{AiAccessMode, ToolName};
    use crate::domain::ai_tools::{
    AskUserArgs, CheckArgs, CheckKind, GitBlameArgs, GitDiffArgs, GrepArgs, ListFilesArgs,
    ReadFileArgs, RequestModeSwitchArgs, SemanticSearchArgs, ToolCall, ToolError, ToolResult,
    ToolScope,
};
    use crate::domain::conversation_mode::ConversationMode;
    use crate::services::ai_tools::testing::*;
    use crate::services::ai_tools::{EmbeddingDeps, execute_tool};

    use super::*;

    #[test]
    fn execute_tool_denies_a_tool_missing_from_a_customized_allowlist() {
        let (repo, docs) = fixture_repo();
        let only_list: HashSet<ToolName> = [ToolName::ListFiles].into_iter().collect();
        let scope = ToolScope::new(&repo, &docs, AiAccessMode::DocsOnly, only_list);

        let err = read(&scope, "intro.adoc").unwrap_err();
        assert!(matches!(err, ToolError::NotAllowed(ToolName::ReadFile)));

        // The other tool in the same customized allowlist still works.
        assert!(list(&scope, None).is_ok());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn execute_tool_denies_semantic_search_missing_from_a_customized_allowlist() {
        let (repo, docs) = fixture_repo();
        let only_list: HashSet<ToolName> =
            [ToolName::ListFiles, ToolName::ReadFile].into_iter().collect();
        let scope = ToolScope::new(&repo, &docs, AiAccessMode::DocsOnly, only_list);

        let err = execute_tool(
            &scope,
            ToolCall::SemanticSearch(SemanticSearchArgs {
                query: "intro".to_string(),
                top_k: None,
            }),
            &EmbeddingDeps::empty(),
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::NotAllowed(ToolName::SemanticSearch)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn tool_call_and_result_round_trip_through_json() {
        let call = ToolCall::ReadFile(ReadFileArgs {
            path: "intro.adoc".to_string(),
            start_line: None,
            end_line: None,
        });
        let json = serde_json::to_string(&call).unwrap();
        assert_eq!(
            json,
            r#"{"tool":"readFile","args":{"path":"intro.adoc","startLine":null,"endLine":null}}"#
        );
        let round_tripped: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, call);

        let result = ToolResult::File {
            content: "= Intro\n".to_string(),
            start_line: 1,
            end_line: 1,
            total_lines: 1,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert_eq!(
            json,
            r#"{"tool":"file","result":{"content":"= Intro\n","startLine":1,"endLine":1,"totalLines":1}}"#
        );
    }

    #[test]
    fn execute_tool_acknowledges_a_mode_switch_request_without_mutating_anything() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps::empty();

        let result = execute_tool(
            &scope,
            ToolCall::RequestModeSwitch(RequestModeSwitchArgs {
                mode: ConversationMode::Plan,
                reason: "just drafting a plan first".to_string(),
            }),
            &deps,
            &[],
        )
        .unwrap();

        assert_eq!(
            result,
            ToolResult::ModeSwitchRequested {
                mode: ConversationMode::Plan,
                reason: "just drafting a plan first".to_string(),
            }
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn execute_tool_rejects_bare_ask_user_without_resume_answer() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps::empty();

        let err = execute_tool(
            &scope,
            ToolCall::AskUser(AskUserArgs {
                title: None,
                questions: vec![crate::domain::ai_tools::AskUserQuestion {
                    id: "q".to_string(),
                    prompt: "Pick one".to_string(),
                    options: vec![
                        crate::domain::ai_tools::AskUserOption {
                            id: "a".to_string(),
                            label: "A".to_string(),
                        },
                        crate::domain::ai_tools::AskUserOption {
                            id: "b".to_string(),
                            label: "B".to_string(),
                        },
                    ],
                    allow_multiple: false,
                }],
            }),
            &deps,
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { tool, .. } if tool == "askUser"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn llm_tool_definitions_includes_all_twenty_one_in_agent_mode_by_default() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let defs = llm_tool_definitions(&scope, ConversationMode::Agent);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "listFiles",
                "readFile",
                "semanticSearch",
                "grep",
                "gitDiff",
                "gitBlame",
                "check",
                "writeFile",
                "editFile",
                "deleteFile",
                "createDirectory",
                "deleteDirectory",
                "move",
                "requestFullRepoAccess",
                "todo",
                "requestModeSwitch",
                "getAsciidocTemplates",
                "skill",
                "askUser",
                "readPlan",
                "updatePlanTodo",
            ]
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn llm_tool_definitions_excludes_a_tool_missing_from_a_customized_allowlist() {
        let (repo, docs) = fixture_repo();
        let only_list: HashSet<ToolName> = [ToolName::ListFiles].into_iter().collect();
        let scope = ToolScope::new(&repo, &docs, AiAccessMode::DocsOnly, only_list);

        let defs = llm_tool_definitions(&scope, ConversationMode::Agent);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["listFiles"]);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn llm_tool_definitions_excludes_mutation_tools_in_plan_and_question_mode() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let plan_names: HashSet<String> = llm_tool_definitions(&scope, ConversationMode::Plan)
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert!(!plan_names.contains("writeFile"));
        assert!(!plan_names.contains("todo"));
        assert!(!plan_names.contains("updatePlanTodo"));
        assert!(plan_names.contains("requestFullRepoAccess"));
        assert!(plan_names.contains("requestModeSwitch"));
        assert!(plan_names.contains("getAsciidocTemplates"));
        assert!(plan_names.contains("skill"));
        assert!(plan_names.contains("askUser"));
        assert!(plan_names.contains("createPlan"));
        assert!(plan_names.contains("updatePlan"));
        assert!(plan_names.contains("readPlan"));

        let question_names: HashSet<String> = llm_tool_definitions(&scope, ConversationMode::Question)
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert!(!question_names.contains("writeFile"));
        assert!(!question_names.contains("requestFullRepoAccess"));
        assert!(question_names.contains("requestModeSwitch"));
        assert!(question_names.contains("getAsciidocTemplates"));
        assert!(question_names.contains("skill"));
        assert!(question_names.contains("askUser"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn llm_tool_definitions_parameters_round_trip_a_realistic_arguments_payload() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let defs = llm_tool_definitions(&scope, ConversationMode::Agent);

        let read_file = defs.iter().find(|d| d.name == "readFile").unwrap();
        assert_eq!(read_file.parameters["required"], serde_json::json!(["path"]));
        let args: ReadFileArgs =
            serde_json::from_value(serde_json::json!({"path": "intro.adoc"})).unwrap();
        assert_eq!(args.path, "intro.adoc");
        assert_eq!(args.start_line, None);
        assert_eq!(args.end_line, None);
        let args: ReadFileArgs = serde_json::from_value(
            serde_json::json!({"path": "intro.adoc", "startLine": 2, "endLine": 10}),
        )
        .unwrap();
        assert_eq!(args.start_line, Some(2));
        assert_eq!(args.end_line, Some(10));

        let list_files = defs.iter().find(|d| d.name == "listFiles").unwrap();
        assert_eq!(list_files.parameters["required"], serde_json::json!([]));
        let args: ListFilesArgs = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(args.path, None);
        assert_eq!(args.depth, None);
        assert_eq!(args.pattern, None);
        let args: ListFilesArgs = serde_json::from_value(
            serde_json::json!({"path": "src", "depth": 2, "pattern": "*.java"}),
        )
        .unwrap();
        assert_eq!(args.depth, Some(2));
        assert_eq!(args.pattern, Some("*.java".to_string()));

        let semantic_search = defs.iter().find(|d| d.name == "semanticSearch").unwrap();
        assert_eq!(semantic_search.parameters["required"], serde_json::json!(["query"]));
        let args: SemanticSearchArgs =
            serde_json::from_value(serde_json::json!({"query": "x", "topK": 3})).unwrap();
        assert_eq!(args.query, "x");
        assert_eq!(args.top_k, Some(3));

        let grep = defs.iter().find(|d| d.name == "grep").unwrap();
        assert_eq!(grep.parameters["required"], serde_json::json!(["pattern"]));
        let args: GrepArgs = serde_json::from_value(serde_json::json!({
            "pattern": "foo\\(",
            "path": null,
            "glob": "*.adoc",
            "caseInsensitive": true,
            "maxResults": 10
        }))
        .unwrap();
        assert_eq!(args.pattern, "foo\\(");
        assert_eq!(args.glob.as_deref(), Some("*.adoc"));
        assert_eq!(args.case_insensitive, Some(true));
        assert_eq!(args.max_results, Some(10));

        let git_diff = defs.iter().find(|d| d.name == "gitDiff").unwrap();
        assert_eq!(git_diff.parameters["required"], serde_json::json!(["path"]));
        let args: GitDiffArgs = serde_json::from_value(
            serde_json::json!({"path": "intro.adoc", "scope": "staged", "commit": null}),
        )
        .unwrap();
        assert_eq!(args.path, "intro.adoc");
        assert_eq!(args.scope.as_deref(), Some("staged"));
        assert_eq!(args.commit, None);

        let git_blame = defs.iter().find(|d| d.name == "gitBlame").unwrap();
        assert_eq!(git_blame.parameters["required"], serde_json::json!(["path"]));
        let args: GitBlameArgs = serde_json::from_value(
            serde_json::json!({"path": "intro.adoc", "startLine": 1, "endLine": 5}),
        )
        .unwrap();
        assert_eq!(args.start_line, Some(1));
        assert_eq!(args.end_line, Some(5));

        let check = defs.iter().find(|d| d.name == "check").unwrap();
        assert_eq!(check.parameters["required"], serde_json::json!(["kind"]));
        assert_eq!(
            check.parameters["properties"]["kind"]["enum"],
            serde_json::json!(["problems", "standards"])
        );
        let args: CheckArgs = serde_json::from_value(
            serde_json::json!({"kind": "problems", "path": "intro.adoc"}),
        )
        .unwrap();
        assert_eq!(args.kind, CheckKind::Problems);
        assert_eq!(args.path.as_deref(), Some("intro.adoc"));

        let standards_args: CheckArgs =
            serde_json::from_value(serde_json::json!({"kind": "standards"})).unwrap();
        assert_eq!(standards_args.kind, CheckKind::Standards);
        assert_eq!(standards_args.path, None);

        assert!(defs.iter().find(|d| d.name == "memory").is_none());

        fs::remove_dir_all(&repo).ok();
    }
}
