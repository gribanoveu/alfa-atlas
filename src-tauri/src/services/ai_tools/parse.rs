//! Turning the model's raw `LlmToolCall` — a tool name plus a JSON string
//! it generated — into a typed `ToolCall`, and rejecting it early when the
//! arguments cannot be honoured.
//!
//! Everything here is defensive against a model that gets the shape almost
//! right: trailing garbage after a complete JSON object is tolerated, a
//! malformed edit reports its array index, and `todo` is one wire-level
//! tool that fans out into two `ToolCall` variants.

use crate::domain::ai_tools::{
    AskUserArgs, CheckArgs, CreateDirectoryArgs, CreatePlanArgs, DeleteDirectoryArgs,
    DeleteFileArgs, EditFileArgs, GetAsciidocTemplatesArgs, GitBlameArgs, GitDiffArgs,
    GrepArgs, ListFilesArgs, MoveArgs, ReadFileArgs, ReadPlanArgs, RequestFullRepoAccessArgs,
    RequestModeSwitchArgs, SemanticSearchArgs, SkillArgs, TodoUpdateArgs, TodoUpdateStatus,
    TodoWriteArgs, ToolCall, ToolError, ToolScope, UpdatePlanArgs, UpdatePlanTodoArgs,
    WriteFileArgs,
};
use crate::domain::llm::LlmToolCall;

use super::resolve::resolve_mutable_docs_path;
use std::collections::HashSet;

/// Parses a model-supplied `LlmToolCall` into a concrete `ToolCall` — the
/// step `domain::llm::LlmToolCall`'s own doc comment names this module as
/// the intended home for. A model can send a `name` this executor doesn't
/// recognize, or `arguments` that don't deserialize into the matching args
/// struct (a hallucinated field, wrong type, plain non-JSON); both are
/// `ToolError` variants meant to be fed back to the model as a `Tool`-role
/// message (see `commands::llm::llm_chat_stream`), not to hard-fail the
/// whole turn — a model recovering from a bad tool call of its own making
/// is normal, expected tool-calling behavior.
pub fn parse_tool_call(call: &LlmToolCall) -> Result<ToolCall, ToolError> {
    match call.name.as_str() {
        "readFile" => lenient_json_object::<ReadFileArgs>(&call.arguments)
            .map(ToolCall::ReadFile)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "listFiles" => lenient_json_object::<ListFilesArgs>(&call.arguments)
            .map(ToolCall::ListFiles)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "semanticSearch" => lenient_json_object::<SemanticSearchArgs>(&call.arguments)
            .map(ToolCall::SemanticSearch)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "grep" => lenient_json_object::<GrepArgs>(&call.arguments)
            .map(ToolCall::Grep)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "gitDiff" => lenient_json_object::<GitDiffArgs>(&call.arguments)
            .map(ToolCall::GitDiff)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "gitBlame" => lenient_json_object::<GitBlameArgs>(&call.arguments)
            .map(ToolCall::GitBlame)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "check" => lenient_json_object::<CheckArgs>(&call.arguments)
            .map(ToolCall::Check)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "writeFile" => lenient_json_object::<WriteFileArgs>(&call.arguments)
            .map(ToolCall::WriteFile)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "editFile" => lenient_json_object::<EditFileArgs>(&call.arguments)
            .map(ToolCall::EditFile)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "deleteFile" => lenient_json_object::<DeleteFileArgs>(&call.arguments)
            .map(ToolCall::DeleteFile)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "createDirectory" => lenient_json_object::<CreateDirectoryArgs>(&call.arguments)
            .map(ToolCall::CreateDirectory)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "deleteDirectory" => lenient_json_object::<DeleteDirectoryArgs>(&call.arguments)
            .map(ToolCall::DeleteDirectory)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "move" => lenient_json_object::<MoveArgs>(&call.arguments)
            .map(ToolCall::Move)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "requestFullRepoAccess" => lenient_json_object::<RequestFullRepoAccessArgs>(&call.arguments)
            .map(ToolCall::RequestFullRepoAccess)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "todo" => parse_todo_call(&call.arguments)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "requestModeSwitch" => lenient_json_object::<RequestModeSwitchArgs>(&call.arguments)
            .map(ToolCall::RequestModeSwitch)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "getAsciidocTemplates" => lenient_json_object::<GetAsciidocTemplatesArgs>(&call.arguments)
            .map(ToolCall::GetAsciidocTemplates)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "skill" => lenient_json_object::<SkillArgs>(&call.arguments)
            .map(ToolCall::Skill)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "askUser" => lenient_json_object::<AskUserArgs>(&call.arguments)
            .and_then(validate_ask_user_args)
            .map(ToolCall::AskUser)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "createPlan" => lenient_json_object::<CreatePlanArgs>(&call.arguments)
            .map(ToolCall::CreatePlan)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "updatePlan" => lenient_json_object::<UpdatePlanArgs>(&call.arguments)
            .map(ToolCall::UpdatePlan)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "readPlan" => lenient_json_object::<ReadPlanArgs>(&call.arguments)
            .map(ToolCall::ReadPlan)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        "updatePlanTodo" => lenient_json_object::<UpdatePlanTodoArgs>(&call.arguments)
            .map(ToolCall::UpdatePlanTodo)
            .map_err(|reason| ToolError::InvalidArguments { tool: call.name.clone(), reason }),
        other => Err(ToolError::UnknownTool(other.to_string())),
    }
}

pub(super) const ASK_USER_MAX_QUESTIONS: usize = 4;

pub(super) const ASK_USER_MIN_OPTIONS: usize = 2;

pub(super) const ASK_USER_MAX_OPTIONS: usize = 6;

/// Structural limits for `askUser` — keeps the mid-turn card usable and
/// stops the model dumping an unbounded questionnaire into one pause.
pub(super) fn validate_ask_user_args(args: AskUserArgs) -> Result<AskUserArgs, String> {
    let n = args.questions.len();
    if n == 0 || n > ASK_USER_MAX_QUESTIONS {
        return Err(format!(
            "questions must have between 1 and {ASK_USER_MAX_QUESTIONS} items, got {n}"
        ));
    }
    let mut seen_q = HashSet::new();
    for (i, q) in args.questions.iter().enumerate() {
        if q.id.trim().is_empty() {
            return Err(format!("questions[{i}].id must be non-empty"));
        }
        if !seen_q.insert(q.id.clone()) {
            return Err(format!("duplicate question id: {}", q.id));
        }
        if q.prompt.trim().is_empty() {
            return Err(format!("questions[{i}].prompt must be non-empty"));
        }
        let opt_n = q.options.len();
        if !(ASK_USER_MIN_OPTIONS..=ASK_USER_MAX_OPTIONS).contains(&opt_n) {
            return Err(format!(
                "questions[{i}].options must have between {ASK_USER_MIN_OPTIONS} and {ASK_USER_MAX_OPTIONS} items, got {opt_n}"
            ));
        }
        let mut seen_o = HashSet::new();
        for (j, o) in q.options.iter().enumerate() {
            if o.id.trim().is_empty() {
                return Err(format!("questions[{i}].options[{j}].id must be non-empty"));
            }
            if o.label.trim().is_empty() {
                return Err(format!("questions[{i}].options[{j}].label must be non-empty"));
            }
            if !seen_o.insert(o.id.clone()) {
                return Err(format!(
                    "questions[{i}] has duplicate option id: {}",
                    o.id
                ));
            }
        }
    }
    Ok(args)
}

/// `todo` is the first tool whose wire name doesn't map 1:1 to a single
/// `ToolCall` variant — it covers two very different argument shapes
/// (`write`'s `tasks: string[]` vs. `update`'s `id`/`status`/`note`) behind
/// one name, deliberately, so the model only ever has to remember one tool
/// (see `llm_tool_definitions`'s `todo` schema). This first deserializes a
/// permissive raw shape (every field but `op` optional), then dispatches
/// and validates the op-specific required fields by hand — the same
/// "recoverable, informative error" contract `lenient_json_object` already
/// gives every other tool, just with one extra branch before it.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawTodoArgs {
    op: String,
    #[serde(default)]
    tasks: Option<Vec<String>>,
    #[serde(default)]
    id: Option<String>,
    // Deliberately a raw `String` rather than `Option<TodoUpdateStatus>`: a
    // typed field would fail JSON deserialization itself on an
    // out-of-enum value like "in_progress", surfacing serde's generic
    // "unknown variant" message instead of the actionable explanation
    // below (models have been observed giving up the turn after that
    // generic message rather than retrying).
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

pub(super) fn parse_todo_call(input: &str) -> Result<ToolCall, String> {
    let raw: RawTodoArgs = lenient_json_object(input)?;
    match raw.op.as_str() {
        "write" => {
            let tasks = raw
                .tasks
                .ok_or_else(|| "op \"write\" requires a non-null `tasks` array".to_string())?;
            if tasks.is_empty() {
                return Err("op \"write\" requires at least one task title in `tasks`".to_string());
            }
            Ok(ToolCall::TodoWrite(TodoWriteArgs { titles: tasks }))
        }
        "update" => {
            let id = raw.id.ok_or_else(|| "op \"update\" requires `id`".to_string())?;
            let raw_status = raw.status.ok_or_else(|| {
                "op \"update\" requires `status` (\"completed\" or \"cancelled\")".to_string()
            })?;
            let status = match raw_status.as_str() {
                "completed" => TodoUpdateStatus::Completed,
                "cancelled" => TodoUpdateStatus::Cancelled,
                "pending" | "in_progress" => {
                    return Err(format!(
                        "`status: \"{raw_status}\"` cannot be set directly — pending/in_progress are runtime-managed: the runtime auto-activates the next pending task the instant the current one is completed or cancelled. Call update with `status: \"completed\"` or `status: \"cancelled\"` instead, or make no call at all if you're just continuing work on the already-active task."
                    ));
                }
                other => {
                    return Err(format!(
                        "`status: \"{other}\"` is not a valid value — expected \"completed\" or \"cancelled\""
                    ));
                }
            };
            Ok(ToolCall::TodoUpdate(TodoUpdateArgs { id, status, note: raw.note }))
        }
        other => Err(format!("unknown todo op: \"{other}\" (expected \"write\" or \"update\")")),
    }
}

/// Parses `arguments` tolerating trailing garbage *after* an otherwise
/// complete, valid JSON object — a real, observed failure mode: some
/// providers stream a tool call's `arguments` as multiple fragments that
/// get concatenated (see `infra::llm_providers::openai_compatible::
/// ToolCallAccumulator`), and at least one has been seen appending a
/// spurious extra fragment after the object already closed (e.g. `{}` then
/// a stray `""`, producing `{}""` — plain `serde_json::from_str` rejects
/// this as "trailing characters" even though the object itself is fine).
///
/// `serde_json::from_str` calls the deserializer's `end()`, which fails on
/// any leftover non-whitespace input; deserializing directly from a
/// `Deserializer` without calling `end()` stops as soon as one complete
/// value has been read and simply ignores whatever follows. This only
/// rescues "valid value + garbage" — a genuinely malformed object (missing
/// braces, invalid syntax partway through) still fails exactly as before,
/// so a model that sends truly broken JSON still gets an honest error to
/// learn from.
///
/// Errors are routed through `serde_path_to_error` rather than plain
/// `serde_json`, so a genuine failure (missing/wrong-typed field) is
/// reported with the JSON path it occurred at — e.g. `edits[1]: missing
/// field \`old\`` — instead of a bare `serde_json::Error`'s `"... at line 1
/// column 7275"`, a byte offset with no indication of *which* element of a
/// batched argument (like `editFile`'s `edits` array) it's inside. A real
/// observed case: a model got one edit right (`old`/`new`) and, several
/// elements later in the same array, typo'd `oldText`/`newText` instead —
/// the byte-offset error gave no way to tell which of the edits was wrong
/// without counting characters by hand; the path does it directly.
pub(super) fn lenient_json_object<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, String> {
    let mut de = serde_json::Deserializer::from_str(input);
    serde_path_to_error::deserialize(&mut de).map_err(|e| e.to_string())
}

/// Path-containment preflight for the chat tool loop — runs before
/// `PendingApproval` so an impossible write (outside the documentation
/// root) never shows a confirmation card. Returns `Ok(())` when the call
/// has no docs-gated path or the path resolves under `docs_root`.
pub fn preflight_tool_call(scope: &ToolScope, call: &LlmToolCall) -> Result<(), ToolError> {
    let parsed = parse_tool_call(call)?;
    match parsed {
        ToolCall::WriteFile(args) => {
            resolve_mutable_docs_path(scope, &args.path)?;
        }
        ToolCall::EditFile(args) => {
            resolve_mutable_docs_path(scope, &args.path)?;
        }
        ToolCall::DeleteFile(args) => {
            resolve_mutable_docs_path(scope, &args.path)?;
        }
        ToolCall::CreateDirectory(args) => {
            resolve_mutable_docs_path(scope, &args.path)?;
        }
        ToolCall::DeleteDirectory(args) => {
            resolve_mutable_docs_path(scope, &args.path)?;
        }
        ToolCall::Move(args) => {
            resolve_mutable_docs_path(scope, &args.path)?;
            resolve_mutable_docs_path(scope, &args.new_path)?;
        }
        ToolCall::Check(args) => {
            if let Some(path) = args.path.as_deref() {
                resolve_mutable_docs_path(scope, path)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::domain::ai_access::AiAccessMode;
    use crate::domain::ai_tools::{
    CheckArgs, CheckKind, CreateDirectoryArgs, DeleteDirectoryArgs, DeleteFileArgs,
    EditFileArgs, FileEdit, GitBlameArgs, GitDiffArgs, GrepArgs, ListFilesArgs, MoveArgs,
    ReadFileArgs, SemanticSearchArgs, SkillArgs, TodoUpdateArgs, TodoUpdateStatus,
    TodoWriteArgs, ToolCall, ToolError, ToolScope, WriteFileArgs,
};
    use crate::domain::conversation_mode::ConversationMode;
    use crate::domain::llm::LlmToolCall;
    use crate::services::ai_tools::testing::*;
    use crate::services::ai_tools::parse_tool_call;

    use super::*;

    #[test]
    fn parse_tool_call_parses_read_file_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "readFile".to_string(),
            arguments: r#"{"path":"intro.adoc"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(parsed, ToolCall::ReadFile(ReadFileArgs { path: "intro.adoc".to_string(), start_line: None, end_line: None }));
    }

    #[test]
    fn parse_tool_call_parses_list_files_args_with_null_path() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "listFiles".to_string(),
            arguments: r#"{"path":null}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(parsed, ToolCall::ListFiles(ListFilesArgs { path: None, depth: None, pattern: None }));
    }

    #[test]
    fn parse_tool_call_parses_list_files_args_with_empty_object() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "listFiles".to_string(),
            arguments: "{}".to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(parsed, ToolCall::ListFiles(ListFilesArgs { path: None, depth: None, pattern: None }));
    }

    /// The exact malformed shape documented on `lenient_json_object`: a
    /// provider's streamed tool-call fragments concatenate into `{}` plus a
    /// stray trailing `""`. Must parse the same as a plain `{}` — root path,
    /// unlimited depth — not error out on the trailing garbage.
    #[test]
    fn parse_tool_call_tolerates_trailing_garbage_after_empty_object() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "listFiles".to_string(),
            arguments: r#"{}"""#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(parsed, ToolCall::ListFiles(ListFilesArgs { path: None, depth: None, pattern: None }));
    }

    #[test]
    fn parse_tool_call_parses_read_file_args_with_line_range() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "readFile".to_string(),
            arguments: r#"{"path":"intro.adoc","startLine":2,"endLine":10}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::ReadFile(ReadFileArgs {
                path: "intro.adoc".to_string(),
                start_line: Some(2),
                end_line: Some(10),
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_list_files_args_with_depth_and_pattern() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "listFiles".to_string(),
            arguments: r#"{"path":"src","depth":2,"pattern":"*.java"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::ListFiles(ListFilesArgs {
                path: Some("src".to_string()),
                depth: Some(2),
                pattern: Some("*.java".to_string()),
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_semantic_search_args_with_top_k() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "semanticSearch".to_string(),
            arguments: r#"{"query":"auth flow","topK":5}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::SemanticSearch(SemanticSearchArgs { query: "auth flow".to_string(), top_k: Some(5) })
        );
    }

    #[test]
    fn parse_tool_call_parses_grep_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "grep".to_string(),
            arguments: r#"{"pattern":"Needle","glob":"*.adoc","caseInsensitive":true,"maxResults":20}"#.to_string(),
        };
        assert_eq!(
            parse_tool_call(&call).unwrap(),
            ToolCall::Grep(GrepArgs {
                pattern: "Needle".to_string(),
                path: None,
                glob: Some("*.adoc".to_string()),
                case_insensitive: Some(true),
                max_results: Some(20),
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_git_diff_and_git_blame_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "gitDiff".to_string(),
            arguments: r#"{"path":"intro.adoc","scope":"staged"}"#.to_string(),
        };
        assert_eq!(
            parse_tool_call(&call).unwrap(),
            ToolCall::GitDiff(GitDiffArgs {
                path: "intro.adoc".to_string(),
                scope: Some("staged".to_string()),
                commit: None,
            })
        );

        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "gitDiff".to_string(),
            arguments: r#"{"path":"intro.adoc","commit":"abc1234"}"#.to_string(),
        };
        assert_eq!(
            parse_tool_call(&call).unwrap(),
            ToolCall::GitDiff(GitDiffArgs {
                path: "intro.adoc".to_string(),
                scope: None,
                commit: Some("abc1234".to_string()),
            })
        );

        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "gitBlame".to_string(),
            arguments: r#"{"path":"intro.adoc","startLine":2,"endLine":10}"#.to_string(),
        };
        assert_eq!(
            parse_tool_call(&call).unwrap(),
            ToolCall::GitBlame(GitBlameArgs {
                path: "intro.adoc".to_string(),
                start_line: Some(2),
                end_line: Some(10),
            })
        );
    }

    #[test]
    fn preflight_rejects_write_outside_documentation_before_execution() {
        let (repo, docs) = fixture_repo();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let call = LlmToolCall {
            id: "c1".into(),
            name: "writeFile".into(),
            arguments: r#"{"path":"src/main.rs","content":"x"}"#.into(),
        };
        let err = preflight_tool_call(&full_repo, &call).unwrap_err();
        assert!(
            matches!(err, ToolError::OutsideDocumentation(_)),
            "got {err:?}"
        );

        let ok = LlmToolCall {
            id: "c2".into(),
            name: "writeFile".into(),
            arguments: r#"{"path":"docs/guide.adoc","content":"= G\n"}"#.into(),
        };
        preflight_tool_call(&full_repo, &ok).unwrap();

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn parse_tool_call_parses_check_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "check".to_string(),
            arguments: r#"{"kind":"problems"}"#.to_string(),
        };
        assert_eq!(
            parse_tool_call(&call).unwrap(),
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: None,
            })
        );

        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "check".to_string(),
            arguments: r#"{"kind":"problems","path":"api/foo.adoc"}"#.to_string(),
        };
        assert_eq!(
            parse_tool_call(&call).unwrap(),
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: Some("api/foo.adoc".to_string()),
            })
        );
    }

    #[test]
    fn parse_tool_call_rejects_unknown_check_kind() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "check".to_string(),
            arguments: r#"{"kind":"docsVsCode"}"#.to_string(),
        };
        let err = parse_tool_call(&call).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { tool, .. } if tool == "check"));
    }

    #[test]
    fn parse_tool_call_parses_write_file_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "writeFile".to_string(),
            arguments: r#"{"path":"guide.adoc","content":"= Guide\n"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::WriteFile(WriteFileArgs {
                path: "guide.adoc".to_string(),
                content: "= Guide\n".to_string(),
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_create_directory_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "createDirectory".to_string(),
            arguments: r#"{"path":"guides/nested"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::CreateDirectory(CreateDirectoryArgs {
                path: "guides/nested".to_string(),
                template: None,
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_create_directory_with_rest_endpoint_template() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "createDirectory".to_string(),
            arguments: r#"{"path":"api/getUser","template":"restEndpoint"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::CreateDirectory(CreateDirectoryArgs {
                path: "api/getUser".to_string(),
                template: Some("restEndpoint".to_string()),
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_delete_file_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "deleteFile".to_string(),
            arguments: r#"{"path":"guide.adoc"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(parsed, ToolCall::DeleteFile(DeleteFileArgs { path: "guide.adoc".to_string() }));
    }

    #[test]
    fn parse_tool_call_parses_delete_directory_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "deleteDirectory".to_string(),
            arguments: r#"{"path":"guides/nested","recursive":true}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::DeleteDirectory(DeleteDirectoryArgs {
                path: "guides/nested".to_string(),
                recursive: Some(true),
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_move_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "move".to_string(),
            arguments: r#"{"path":"old.adoc","newPath":"new.adoc"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::Move(MoveArgs {
                path: "old.adoc".to_string(),
                new_path: "new.adoc".to_string(),
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_edit_file_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "editFile".to_string(),
            arguments: r#"{"path":"guide.adoc","edits":[{"old":"a","new":"b"}]}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::EditFile(EditFileArgs {
                path: "guide.adoc".to_string(),
                edits: vec![FileEdit { old: "a".to_string(), new: "b".to_string() }],
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_request_full_repo_access_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "requestFullRepoAccess".to_string(),
            arguments: r#"{"reason":"need to check the config schema"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::RequestFullRepoAccess(RequestFullRepoAccessArgs {
                reason: "need to check the config schema".to_string(),
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_request_mode_switch_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "requestModeSwitch".to_string(),
            arguments: r#"{"mode":"agent","reason":"user asked to implement the change"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::RequestModeSwitch(RequestModeSwitchArgs {
                mode: ConversationMode::Agent,
                reason: "user asked to implement the change".to_string(),
            })
        );
    }

    #[test]
    fn parse_tool_call_parses_ask_user_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "askUser".to_string(),
            arguments: r#"{"title":"Format","questions":[{"id":"fmt","prompt":"Which format?","options":[{"id":"adoc","label":"AsciiDoc"},{"id":"md","label":"Markdown"}],"allowMultiple":false}]}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        match parsed {
            ToolCall::AskUser(args) => {
                assert_eq!(args.title.as_deref(), Some("Format"));
                assert_eq!(args.questions.len(), 1);
                assert_eq!(args.questions[0].id, "fmt");
                assert_eq!(args.questions[0].options.len(), 2);
                assert!(!args.questions[0].allow_multiple);
            }
            other => panic!("expected AskUser, got {other:?}"),
        }
    }

    #[test]
    fn parse_tool_call_parses_skill_search_args() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "skill".to_string(),
            arguments: r#"{"op":"search","query":"REST method folder"}"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(
            parsed,
            ToolCall::Skill(SkillArgs {
                op: "search".to_string(),
                query: Some("REST method folder".to_string()),
                name: None,
                path: None,
            })
        );
    }

    #[test]
    fn parse_tool_call_rejects_ask_user_with_too_few_options() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "askUser".to_string(),
            arguments: r#"{"questions":[{"id":"q","prompt":"Only one?","options":[{"id":"a","label":"A"}]}]}"#.to_string(),
        };
        let err = parse_tool_call(&call).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { tool, .. } if tool == "askUser"));
    }

    #[test]
    fn parse_tool_call_rejects_unknown_tool_name() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "moveFile".to_string(),
            arguments: "{}".to_string(),
        };
        let err = parse_tool_call(&call).unwrap_err();
        assert!(matches!(err, ToolError::UnknownTool(name) if name == "moveFile"));
    }

    #[test]
    fn parse_tool_call_rejects_malformed_arguments_json() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "readFile".to_string(),
            arguments: "{not json}".to_string(),
        };
        let err = parse_tool_call(&call).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { tool, .. } if tool == "readFile"));
    }

    /// Regression test for a real observed failure: a model sent an
    /// `editFile` call with two edits, the first correct (`old`/`new`), the
    /// second typo'd as `oldText`/`newText`. Before `serde_path_to_error`,
    /// this surfaced as a bare `"missing field \`old\` at line 1 column
    /// 7275"` — technically correct but useless for a model to act on: no
    /// indication of *which* edit (out of a much longer batch) the missing
    /// field was in. The path-annotated error must name the array index.
    #[test]
    fn parse_tool_call_reports_the_array_index_of_a_malformed_edit() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "editFile".to_string(),
            arguments: r#"{"path":"x.adoc","edits":[{"old":"a","new":"b"},{"oldText":"c","newText":"d"}]}"#
                .to_string(),
        };
        let err = parse_tool_call(&call).unwrap_err();
        match err {
            ToolError::InvalidArguments { tool, reason } => {
                assert_eq!(tool, "editFile");
                assert!(reason.contains("edits[1]"), "reason should name the array index: {reason}");
                assert!(reason.contains("old"), "reason should name the missing field: {reason}");
            }
            other => panic!("expected InvalidArguments, got {other:?}"),
        }
    }

    #[test]
    fn parse_tool_call_tolerates_trailing_garbage_after_a_complete_object() {
        // Real observed case: a provider streamed `listFiles` arguments as
        // fragments that concatenated into `{}""` — a complete, valid empty
        // object followed by a stray extra fragment. Strict `serde_json`
        // rejects this ("trailing characters"); the lenient fallback should
        // still recover the real (empty) arguments rather than surfacing an
        // error the user can do nothing about.
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "listFiles".to_string(),
            arguments: "{}\"\"".to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(parsed, ToolCall::ListFiles(ListFilesArgs { path: None, depth: None, pattern: None }));
    }

    #[test]
    fn parse_tool_call_tolerates_trailing_garbage_after_a_populated_object() {
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "readFile".to_string(),
            arguments: r#"{"path":"intro.adoc"}garbage"#.to_string(),
        };
        let parsed = parse_tool_call(&call).unwrap();
        assert_eq!(parsed, ToolCall::ReadFile(ReadFileArgs { path: "intro.adoc".to_string(), start_line: None, end_line: None }));
    }

    #[test]
    fn parse_tool_call_still_rejects_arguments_that_are_invalid_from_the_start() {
        // The lenient fallback only rescues "valid value + trailing junk" —
        // JSON that's broken from the very first token must still error,
        // so the model still gets an honest, actionable error to learn
        // from rather than silently defaulting to empty arguments.
        let call = LlmToolCall {
            id: "call_1".to_string(),
            name: "readFile".to_string(),
            arguments: "{not json at all".to_string(),
        };
        let err = parse_tool_call(&call).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { tool, .. } if tool == "readFile"));
    }

    #[test]
    fn parse_tool_call_dispatches_todo_write_and_todo_update() {
        let write_call = LlmToolCall {
            id: "1".to_string(),
            name: "todo".to_string(),
            arguments: r#"{"op":"write","tasks":["Найти контроллер","Найти сервис"]}"#.to_string(),
        };
        assert_eq!(
            parse_tool_call(&write_call).unwrap(),
            ToolCall::TodoWrite(TodoWriteArgs {
                titles: vec!["Найти контроллер".to_string(), "Найти сервис".to_string()]
            }),
        );

        let update_call = LlmToolCall {
            id: "2".to_string(),
            name: "todo".to_string(),
            arguments: r#"{"op":"update","id":"t2","status":"completed","note":"endpoint в UserController.java:45"}"#
                .to_string(),
        };
        assert_eq!(
            parse_tool_call(&update_call).unwrap(),
            ToolCall::TodoUpdate(TodoUpdateArgs {
                id: "t2".to_string(),
                status: TodoUpdateStatus::Completed,
                note: Some("endpoint в UserController.java:45".to_string()),
            }),
        );
    }

    #[test]
    fn parse_tool_call_rejects_todo_with_an_out_of_enum_status() {
        let call = LlmToolCall {
            id: "1".to_string(),
            name: "todo".to_string(),
            arguments: r#"{"op":"update","id":"t1","status":"in_progress"}"#.to_string(),
        };
        let err = parse_tool_call(&call).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { tool, .. } if tool == "todo"));
    }

    #[test]
    fn parse_tool_call_rejects_unknown_todo_op() {
        let call = LlmToolCall {
            id: "1".to_string(),
            name: "todo".to_string(),
            arguments: r#"{"op":"read"}"#.to_string(),
        };
        let err = parse_tool_call(&call).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { tool, .. } if tool == "todo"));
    }
}
