//! Executor for the read-only tools a future AI harness will call. This is
//! the enforcement point for `AiAccessMode`: every function here resolves
//! containment against `scope.root` via `domain::paths` — the same
//! primitives `services::docs_fs` uses — so a caller can never widen access
//! by passing an unexpected path, only by the `ToolScope` itself having been
//! constructed with the wider root.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, TryLockError};

use crate::commands::embeddings::{
    attach_embedding_index, attach_index_store, ensure_provider, resolve_index_paths,
    EmbeddingIndexSlot, EmbeddingProviderSlot, EmbeddingSyncGuard, IndexStoreSlot,
};
use crate::domain::ai_access::{default_allowed_tools, AiAccessMode, ToolName};
use crate::domain::ai_tools::{
    AsciidocTemplateEntry, CheckArgs, CheckKind, CreateDirectoryArgs, CreatePlanArgs,
    DeleteDirectoryArgs, DeleteFileArgs, EditFileArgs, FileDiffStats, FileEdit,
    GetAsciidocTemplatesArgs, GitBlameArgs, GitDiffArgs, GrepArgs, ListFilesArgs,
    MatchSource, MoveArgs, ReadFileArgs, ReadPlanArgs, RequestFullRepoAccessArgs,
    RequestModeSwitchArgs, AskUserArgs, SemanticSearchArgs, SemanticSearchMeta,
    SemanticSearchPayload, SkillArgs, Task, TodoStatus, TodoUpdateArgs, TodoUpdateStatus,
    TodoWriteArgs, ToolCall, ToolError, ToolFileEntry, ToolMatch, ToolResult, ToolScope,
    UpdatePlanArgs, UpdatePlanTodoArgs, WriteFileArgs,
};
use crate::domain::asciidoc_element_templates::{
    find_many as find_asciidoc_templates, ASCIIDOC_ELEMENT_TEMPLATES,
};
use crate::domain::chunk_index::{qualified_name_for, ChunkMetadata};
use crate::domain::conversation_mode::{mode_tools, ConversationMode};
use crate::domain::git::GitDiffScope;
use crate::domain::llm::{
    ChatRequest, LlmMessage, LlmProvider, LlmRole, LlmToolCall, LlmToolDefinition,
};
use crate::domain::paths;
use crate::domain::project_config::{ProjectConfig, ProjectError, TreeNode, UpdatedReference};
use crate::domain::repo_index::{FileId, Symbol};
use crate::domain::search_query::{
    extract_search_tokens, lexical_token_weight, path_segment_matches, symbol_name_matches_token,
    weak_search_hint, MatchTightness, SearchMetaInput,
};
use crate::domain::workspace_index::DocumentId;
use crate::infra::{embedding_credentials_store, embedding_providers, project_store, workspace_scanner};
use crate::services::chunk_builder::ChunkIndex;
use crate::services::chunk_text::resolve_text;
use crate::services::docs_search;
use crate::services::reference_rewrite;
use crate::services::repo_index::RepositoryIndex;
use crate::services::workspace_index::WorkspaceIndex;
use crate::services::{
    agent_memory, diagnostics, docs_fs, embedding_config, git_ops, project_open, standards,
    standards_prefs, text_diff,
};

const DEFAULT_TOP_K: usize = 10;
const MAX_TOP_K: usize = 50;
/// Cap on how many characters of matched text land in a `ToolMatch.snippet`
/// — keeps a large chunk's (up to 16KB) full text from blowing up the
/// response payload.
const SNIPPET_MAX_CHARS: usize = 500;
/// Hard cap on total tasks in a todo list, enforced by `todo_write` — a
/// `write` that would exceed this fails outright (see
/// `ToolError::TooManyTasks`) rather than silently truncating.
const MAX_TODO_TASKS: usize = 20;
/// Cap on how many lines a single `gitBlame` call may cover — keeps the
/// tool-message payload bounded for large files. Ranges past this are
/// clamped and flagged `truncated: true`.
const MAX_BLAME_LINES: u32 = 400;
/// Cap on how many diagnostics a single `check` call may return — keeps the
/// tool-message payload bounded for a large docs tree.
const MAX_CHECK_DIAGNOSTICS: usize = 200;
/// Cap on how many method-folder results a single `check` (`kind:
/// "standards"`) call may return — same rationale as
/// `MAX_CHECK_DIAGNOSTICS`. Failing folders are kept first (see
/// `check_doc_standards`), so a truncated response still surfaces the
/// folders most worth the model's attention.
const MAX_STANDARDS_FOLDERS: usize = 100;

/// The embedding/chunk/repo-index/workspace-index state `SemanticSearch`/
/// `Move` need to reach — `execute_tool` is otherwise a pure function with
/// no access to Tauri-managed state. Mirrors exactly what
/// `commands::embeddings::embedding_sync` already receives as
/// `State<'_, Arc<T>>` params; `commands::ai_tools::ai_execute_tool`/
/// `commands::llm::llm_chat_stream`/`llm_chat_stream_resume` each clone
/// their own `State`s into this struct before calling `execute_tool`.
/// `workspace_index` was added for `move_path`'s reference-rewrite step
/// (`services::reference_rewrite::rewrite_references`) — the name predates
/// that and is now a bit imprecise, but this is the one established
/// extension point for a new tool needing more Tauri-managed state, not
/// worth a second threading mechanism for one field.
pub struct EmbeddingDeps {
    pub repo_index: Arc<RepositoryIndex>,
    pub chunk_index: Arc<ChunkIndex>,
    pub embedding_index: Arc<EmbeddingIndexSlot>,
    pub index_store: Arc<IndexStoreSlot>,
    pub embedding_provider: Arc<EmbeddingProviderSlot>,
    pub sync_guard: Arc<EmbeddingSyncGuard>,
    pub workspace_index: Arc<WorkspaceIndex>,
    /// `EditFile`'s fast-apply fallback capability — the `(provider, model)`
    /// pair already resolved for the surrounding chat turn, reused rather
    /// than resolving a second one just for this. `None` disables the
    /// fallback entirely (`edit_file` then behaves exactly as it always
    /// did: a plain deterministic `EditTextNotFound`/`EditTextAmbiguous` on
    /// a non-exact match). `commands::llm::run_tool_loop` is the one caller
    /// that sets this; `commands::ai_tools::ai_execute_tool` (a standalone
    /// endpoint with no chat turn to reuse a resolved provider from) leaves
    /// it `None`. Reuses this struct rather than adding a second
    /// threading mechanism for one field — see this struct's own doc
    /// comment above, which already establishes that precedent for `Move`.
    pub fast_apply: Option<(Arc<dyn LlmProvider>, String)>,
    /// The user's currently-open editor tab, if any — `FileId`-space
    /// (already converted from the frontend's docs-root-relative
    /// `EditorTab.path` by `commands::llm::llm_chat_stream`/
    /// `llm_chat_stream_resume`, the same conversion
    /// `embedding_set_priority_files` already establishes as precedent).
    /// `semantic_search` uses this to boost chunks from files related to it
    /// (via `related_files`) — `None` disables the boost entirely, same as
    /// today's unboosted ranking. `commands::ai_tools::ai_execute_tool` (no
    /// chat turn, no editor context) leaves it `None`, same as `fast_apply`.
    pub active_file: Option<FileId>,
}

#[cfg(test)]
impl EmbeddingDeps {
    /// Fresh, empty instances of every slot — for `ReadFile`/`ListFiles`
    /// tests (which never touch these) and as a base for `SemanticSearch`/
    /// `Move` tests that need to populate specific state.
    pub fn empty() -> Self {
        Self {
            repo_index: Arc::new(RepositoryIndex::new()),
            chunk_index: Arc::new(ChunkIndex::new()),
            embedding_index: Arc::new(EmbeddingIndexSlot::new(None)),
            index_store: Arc::new(IndexStoreSlot::new(None)),
            embedding_provider: Arc::new(EmbeddingProviderSlot::new(None)),
            sync_guard: Arc::new(EmbeddingSyncGuard::new(())),
            workspace_index: Arc::new(WorkspaceIndex::new(
                crate::infra::parsers::registry::ParserRegistry::new(),
            )),
            fast_apply: None,
            active_file: None,
        }
    }
}

/// Single entry point for the harness: one allowlist check (via
/// `scope.allows`), one place to serialize a call/result at the LLM
/// boundary (`ToolCall`/`ToolResult` both derive `serde`), and — later —
/// one place to log every tool invocation (not wired up yet).
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
        ToolCall::ReadFile(args) => read_file(scope, args).map(|slice| ToolResult::File {
            content: slice.content,
            start_line: slice.start_line,
            end_line: slice.end_line,
            total_lines: slice.total_lines,
        }),
        ToolCall::ListFiles(args) => list_files(scope, args).map(ToolResult::FileList),
        ToolCall::SemanticSearch(args) => {
            semantic_search(scope, args, deps).map(ToolResult::SemanticSearchResults)
        }
        ToolCall::Grep(args) => grep(scope, args),
        ToolCall::GitDiff(args) => git_diff(scope, args),
        ToolCall::GitBlame(args) => git_blame(scope, args),
        ToolCall::Check(args) => check(scope, args, deps),
        ToolCall::WriteFile(args) => {
            write_file(scope, args, deps).map(|(path, diff)| ToolResult::FileWritten { path, diff })
        }
        ToolCall::EditFile(args) => edit_file(scope, args, deps.fast_apply.as_ref(), deps)
            .map(|(path, diff)| ToolResult::FileEdited { path, diff }),
        ToolCall::DeleteFile(args) => delete_file(scope, args, deps)
            .map(|(path, diff)| ToolResult::FileDeleted { path, diff }),
        ToolCall::CreateDirectory(args) => create_directory(scope, args, deps).map(
            |(path, template, created_files)| ToolResult::DirectoryCreated {
                path,
                template,
                created_files,
            },
        ),
        ToolCall::DeleteDirectory(args) => {
            delete_directory(scope, args, deps).map(|path| ToolResult::DirectoryDeleted { path })
        }
        ToolCall::Move(args) => {
            move_path(scope, args, deps).map(|(from, to, updated_files)| ToolResult::Moved {
                from,
                to,
                updated_files,
            })
        }
        ToolCall::RequestFullRepoAccess(_args) => set_access_mode(AiAccessMode::FullRepo)
            .map(|()| ToolResult::AccessModeChanged { mode: AiAccessMode::FullRepo })
            .map_err(ToolError::from),
        ToolCall::TodoWrite(args) => todo_write(todos, args).map(ToolResult::TodoWritten),
        ToolCall::TodoUpdate(args) => todo_update(todos, args).map(ToolResult::TodoUpdated),
        ToolCall::Memory(_) => Err(ToolError::InvalidArguments {
            tool: "memory".to_string(),
            reason: "the memory tool was removed; long-term memory is managed automatically by the harness".to_string(),
        }),
        // No state to mutate — `ConversationMode` isn't persisted anywhere
        // server-side (see `domain::conversation_mode`'s doc comment); this
        // is a pure acknowledgement the frontend reacts to once the call
        // settles, same as `commands::llm::run_tool_loop` deliberately does
        // *not* re-scope mid-round for this tool the way it does for
        // `RequestFullRepoAccess`.
        ToolCall::RequestModeSwitch(args) => {
            Ok(ToolResult::ModeSwitchRequested { mode: args.mode, reason: args.reason })
        }
        ToolCall::GetAsciidocTemplates(args) => Ok(get_asciidoc_templates(args)),
        ToolCall::Skill(args) => execute_skill(args),
        // Never produced here — answers come from `ToolCallDecision::answer`
        // on `llm_chat_stream_resume`. Calling via bare `ai_execute_tool`
        // without a user answer is a programming error, not a model recovery
        // case (the model never sees this path for a well-formed pause).
        ToolCall::AskUser(_) => Err(ToolError::InvalidArguments {
            tool: "askUser".to_string(),
            reason: "askUser must be answered via resume, not execute_tool".to_string(),
        }),
        ToolCall::CreatePlan(args) => create_plan(args),
        ToolCall::UpdatePlan(args) => update_plan(args),
        ToolCall::ReadPlan(args) => read_plan(args),
        ToolCall::UpdatePlanTodo(args) => update_plan_todo(args),
    }
}

/// Executes `getAsciidocTemplates` — a pure in-memory lookup against the
/// fixed `domain::asciidoc_element_templates::ASCIIDOC_ELEMENT_TEMPLATES`
/// catalog, so unlike almost every other tool here this cannot fail: an
/// empty or entirely-unmatched `ids` just yields an empty/`not_found`-only
/// result rather than a `ToolError`.
fn get_asciidoc_templates(args: GetAsciidocTemplatesArgs) -> ToolResult {
    let (found, not_found) = find_asciidoc_templates(&args.ids);
    let templates = found
        .into_iter()
        .map(|t| AsciidocTemplateEntry {
            id: t.id.to_string(),
            label: t.label.to_string(),
            category: t.category.to_string(),
            template: t.template.to_string(),
        })
        .collect();
    ToolResult::AsciidocTemplates { templates, not_found }
}

fn execute_skill(args: SkillArgs) -> Result<ToolResult, ToolError> {
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

/// Correlation info available at a real (non-test) call site, threaded
/// through to the persisted log row — see `domain::tool_call_log::
/// ToolCallLogRow` for what each field means. Built fresh by each caller
/// from whatever it already has on hand (`commands::ai_tools::
/// ai_execute_tool` has no chat turn to draw `round`/`provider_id`/`model`
/// from; `commands::llm::run_tool_loop` does).
pub struct ToolCallLogContext {
    pub enabled: bool,
    pub source: &'static str,
    pub round: Option<u32>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
}

/// `execute_tool`, plus a redacted entry written to the persisted tool-call
/// log (`infra::tool_call_log`) once the call settles. Deliberately a thin
/// wrapper rather than logging inside `execute_tool` itself: `execute_tool`
/// has ~30 direct call sites in this module's own test suite, all of which
/// exercise it as the pure, I/O-free function it already is — folding
/// logging into it would force every one of those tests to also touch a
/// SQLite file. The two real callers (`commands::ai_tools::ai_execute_tool`,
/// `commands::llm::run_tool_loop`) call this instead.
pub fn execute_tool_logged(
    scope: &ToolScope,
    call: ToolCall,
    deps: &EmbeddingDeps,
    todos: &[Task],
    log_ctx: &ToolCallLogContext,
) -> Result<ToolResult, ToolError> {
    let tool = crate::infra::tool_call_log::tool_label(&call);
    let args_json = crate::infra::tool_call_log::redact_args(&call);
    let memory_op = match &call {
        ToolCall::Memory(args) => Some(args.op.clone()),
        _ => None,
    };
    let started = std::time::Instant::now();
    let result = execute_tool(scope, call, deps, todos);
    let duration_ms = started.elapsed().as_millis() as i64;

    crate::infra::tool_call_log::log_call(
        log_ctx.enabled,
        crate::infra::tool_call_log::ToolCallLogEntry {
            repo_root: scope.repo_root.display().to_string(),
            source: log_ctx.source.to_string(),
            round: log_ctx.round,
            provider_id: log_ctx.provider_id.clone(),
            model: log_ctx.model.clone(),
            tool,
            args_json,
            status: if result.is_ok() { "ok" } else { "error" }.to_string(),
            error_message: result.as_ref().err().map(ToString::to_string),
            result_json: result
                .as_ref()
                .ok()
                .map(|r| crate::infra::tool_call_log::redact_result(r, memory_op.as_deref())),
            duration_ms,
        },
    );

    result
}

fn todo_write(todos: &[Task], args: TodoWriteArgs) -> Result<Vec<Task>, ToolError> {
    let adding = args.titles.len();
    if todos.len() + adding > MAX_TODO_TASKS {
        return Err(ToolError::TooManyTasks {
            current: todos.len(),
            adding,
            max: MAX_TODO_TASKS,
        });
    }
    let mut next_id = todos.len();
    let mut updated = todos.to_vec();
    for title in args.titles {
        next_id += 1;
        updated.push(Task {
            id: format!("t{next_id}"),
            title,
            status: TodoStatus::Pending,
            note: None,
        });
    }
    Ok(enforce_todo_invariant(updated))
}

fn todo_update(todos: &[Task], args: TodoUpdateArgs) -> Result<Vec<Task>, ToolError> {
    let mut updated = todos.to_vec();
    let task = updated
        .iter_mut()
        .find(|t| t.id == args.id)
        .ok_or_else(|| ToolError::TaskNotFound(args.id.clone()))?;
    task.status = args.status.into();
    if let Some(note) = args.note {
        task.note = Some(note);
    }
    Ok(enforce_todo_invariant(updated))
}

fn create_plan(args: CreatePlanArgs) -> Result<ToolResult, ToolError> {
    let todos: Vec<(String, String)> = args
        .todos
        .into_iter()
        .map(|t| (t.id, t.content))
        .collect();
    let record = crate::services::plans::create_plan(
        args.name,
        args.overview,
        args.plan,
        todos,
        None,
    )?;
    Ok(ToolResult::PlanCreated {
        plan_id: record.id,
        name: record.name,
        overview: record.overview,
        todo_count: record.todos.len() as u32,
        todos: record.todos,
    })
}

fn update_plan(args: UpdatePlanArgs) -> Result<ToolResult, ToolError> {
    let todos = args.todos.map(|list| {
        list.into_iter()
            .map(|t| (t.id, t.content))
            .collect::<Vec<_>>()
    });
    let record = crate::services::plans::update_plan(
        &args.plan_id,
        args.name,
        args.overview,
        args.plan,
        todos,
    )?;
    Ok(ToolResult::PlanUpdated {
        plan_id: record.id,
        name: record.name,
        overview: record.overview,
        todo_count: record.todos.len() as u32,
        todos: record.todos,
    })
}

fn read_plan(args: ReadPlanArgs) -> Result<ToolResult, ToolError> {
    let record = crate::services::plans::read_plan(&args.plan_id)?;
    Ok(ToolResult::PlanRead {
        plan_id: record.id,
        name: record.name,
        overview: record.overview,
        plan: record.plan,
        todos: record.todos,
    })
}

fn update_plan_todo(args: UpdatePlanTodoArgs) -> Result<ToolResult, ToolError> {
    let record = crate::services::plans::update_plan_todo(
        &args.plan_id,
        &args.id,
        args.status,
        args.note,
    )?;
    Ok(ToolResult::PlanTodoUpdated {
        plan_id: record.id,
        todos: record.todos,
    })
}

/// The one shared invariant-enforcement function, run at the end of both
/// `todo_write` (a fresh append may leave the whole list without an
/// `InProgress` task — e.g. the very first write ever) and `todo_update`
/// (completing/cancelling the current task always does). At most one
/// `InProgress` task ever exists; when none does and at least one
/// `Pending` task remains, the first one (lowest id / earliest in list
/// order, since ids are assigned sequentially and the list is
/// append-only) is promoted. A no-op when an `InProgress` task already
/// exists (e.g. `todo_write` appending onto an already-active list) or
/// when no `Pending` task remains (list fully completed/cancelled).
fn enforce_todo_invariant(mut tasks: Vec<Task>) -> Vec<Task> {
    let has_in_progress = tasks.iter().any(|t| t.status == TodoStatus::InProgress);
    if !has_in_progress {
        if let Some(next) = tasks.iter_mut().find(|t| t.status == TodoStatus::Pending) {
            next.status = TodoStatus::InProgress;
        }
    }
    tasks
}

/// Persists a new `AiAccessMode` for the currently open project — shared by
/// the manual `commands::ai_tools::ai_set_access_mode` toggle and the
/// `RequestFullRepoAccess` tool, so a mode change behaves identically
/// regardless of which path triggered it. Preserves any existing
/// `ai_allowed_tools` override rather than resetting it.
pub fn set_access_mode(mode: AiAccessMode) -> Result<(), ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let mut config = project_store::load(&opened.root)?
        .unwrap_or_else(|| ProjectConfig::new(opened.docs_root.clone()));
    config.ai_access_mode = mode;
    project_store::save(&opened.root, &config)
}

/// Tool names the currently open project has persisted as "don't ask for
/// confirmation again" (`ProjectConfig::ai_auto_approved_tools`) — read by
/// the frontend once per chat panel mount to seed its in-memory trusted-tool
/// set, so a choice made in one chat carries into every later chat on the
/// same repo. Empty when the project has never customized this (matches the
/// `None` default), not an error.
pub fn auto_approved_tools() -> Result<HashSet<ToolName>, ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let config = project_store::load(&opened.root)?
        .unwrap_or_else(|| ProjectConfig::new(opened.docs_root.clone()));
    Ok(config.ai_auto_approved_tools.unwrap_or_default().into_iter().collect())
}

/// Persists (or revokes) one tool's "always allow" status for the currently
/// open project — the backend counterpart to the approval card's "Разрешать
/// всегда" button. Only ever changes whether a *future* call still pauses
/// for confirmation; it never widens `ai_allowed_tools`, so a tool the
/// project has otherwise disallowed stays disallowed regardless.
pub fn set_tool_auto_approved(tool: ToolName, auto_approved: bool) -> Result<(), ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let mut config = project_store::load(&opened.root)?
        .unwrap_or_else(|| ProjectConfig::new(opened.docs_root.clone()));
    let mut set: HashSet<ToolName> = config.ai_auto_approved_tools.unwrap_or_default().into_iter().collect();
    if auto_approved {
        set.insert(tool);
    } else {
        set.remove(&tool);
    }
    config.ai_auto_approved_tools = Some(set.into_iter().collect());
    project_store::save(&opened.root, &config)
}

/// Tool names the currently open project's `ai_allowed_tools` currently
/// resolves to — the customized set if one was ever saved, else `mode`'s
/// default (mirrors `scope_for_config`'s own resolution exactly, so what
/// this reports is always what `execute_tool` actually enforces).
pub fn allowed_tools() -> Result<HashSet<ToolName>, ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let config = load_project_config_migrated(&opened.root, &opened.docs_root)?;
    Ok(config
        .ai_allowed_tools
        .clone()
        .unwrap_or_else(|| default_allowed_tools(config.ai_access_mode).into_iter().collect())
        .into_iter()
        .collect())
}

/// Persists (or revokes) one tool's membership in `ai_allowed_tools` for the
/// currently open project — the backend counterpart to a new Settings UI
/// checkbox. Seeds the customized set from the current default (rather than
/// starting from empty) the first time any tool is toggled, so unchecking
/// one tool doesn't silently disallow every other tool too.
pub fn set_tool_allowed(tool: ToolName, allowed: bool) -> Result<(), ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let mut config = load_project_config_migrated(&opened.root, &opened.docs_root)?;
    let mut set: HashSet<ToolName> = config
        .ai_allowed_tools
        .clone()
        .map(|v| v.into_iter().collect())
        .unwrap_or_else(|| default_allowed_tools(config.ai_access_mode));
    if allowed {
        set.insert(tool);
    } else {
        set.remove(&tool);
    }
    config.ai_allowed_tools = Some(set.into_iter().collect());
    project_store::save(&opened.root, &config)
}

/// `ToolName` variants introduced by the plan-mode feature. A project whose
/// `ai_allowed_tools` was customized (Settings → Permissions) before these
/// variants existed cannot have intentionally revoked them — they weren't
/// yet options to revoke. See `migrate_plan_tools_into_allowlist`.
const PLAN_TOOLS_MIGRATION: [ToolName; 4] = [
    ToolName::CreatePlan,
    ToolName::UpdatePlan,
    ToolName::ReadPlan,
    ToolName::UpdatePlanTodo,
];

/// Same backfill reason as `PLAN_TOOLS_MIGRATION` for the Agent Skills router.
const SKILL_TOOL_MIGRATION: [ToolName; 1] = [ToolName::Skill];

/// Backfills `config.ai_allowed_tools` with any `PLAN_TOOLS_MIGRATION` tool
/// missing from an already-customized list, so a project saved before this
/// feature shipped doesn't permanently lose access to it — `ToolName`
/// variants added later never automatically widen a customized allowlist
/// (see `default_allowed_tools`'s doc comment), so without this a
/// customized project would need the user to manually re-enable each new
/// tool in Settings. No-op when `ai_allowed_tools` is `None` — an
/// uncustomized project already resolves through `default_allowed_tools`,
/// which includes these. Returns whether anything changed, so the caller
/// knows whether to persist.
fn migrate_plan_tools_into_allowlist(config: &mut ProjectConfig) -> bool {
    let Some(list) = config.ai_allowed_tools.as_mut() else {
        return false;
    };
    let mut changed = false;
    for tool in PLAN_TOOLS_MIGRATION.iter().chain(SKILL_TOOL_MIGRATION.iter()) {
        if !list.contains(tool) {
            list.push(*tool);
            changed = true;
        }
    }
    changed
}

/// Shared "load this project's config, catching it up on any pending
/// allowlist migration" used by every call site that resolves
/// `ai_allowed_tools` (`allowed_tools`, `set_tool_allowed`, `current_scope`)
/// — replaces their previous direct
/// `project_store::load(...).unwrap_or_else(...)` so a project's allowlist
/// only needs to catch up once, on whichever of the three runs first.
/// Persists immediately when migration changed anything (mirrors
/// `infra::chat_store`'s ALTER-on-open precedent, just at the project.json
/// layer).
fn load_project_config_migrated(root: &str, docs_root_fallback: &str) -> Result<ProjectConfig, ProjectError> {
    let mut config = project_store::load(root)?.unwrap_or_else(|| ProjectConfig::new(docs_root_fallback));
    if migrate_plan_tools_into_allowlist(&mut config) {
        project_store::save(root, &config)?;
    }
    Ok(config)
}

/// Resolves a `ToolScope` from a project's persisted config — the one place
/// that turns "user hasn't customized anything" into `mode`'s default
/// allowlist, and a customized list into the authoritative one.
pub fn scope_for_config(repo_root: &Path, docs_root: &Path, config: &ProjectConfig) -> ToolScope {
    let allowed: HashSet<ToolName> = config
        .ai_allowed_tools
        .clone()
        .map(|v| v.into_iter().collect())
        .unwrap_or_else(|| default_allowed_tools(config.ai_access_mode));
    ToolScope::new(repo_root, docs_root, config.ai_access_mode, allowed)
}

/// Resolves a `ToolScope` for whichever project is currently open, without
/// the caller (the IPC command) supplying any path — this is what lets the
/// frontend call `ai_execute_tool` knowing nothing about `docsRoot`/
/// `repoRoot`/the access mode. Reuses the same backend-authoritative source
/// `commands::project::get_project` already uses at startup restore;
/// `project_open::get_project()` alone doesn't expose `ai_access_mode`/
/// `ai_allowed_tools` (it discards the rest of `ProjectConfig`), so those
/// are loaded separately here.
pub fn current_scope() -> Result<ToolScope, ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let config = load_project_config_migrated(&opened.root, &opened.docs_root)?;
    Ok(scope_for_config(
        Path::new(&opened.root),
        Path::new(&opened.docs_root),
        &config,
    ))
}

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
            .and_then(|args| validate_ask_user_args(args).map_err(|reason| reason))
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

const ASK_USER_MAX_QUESTIONS: usize = 4;
const ASK_USER_MIN_OPTIONS: usize = 2;
const ASK_USER_MAX_OPTIONS: usize = 6;

/// Structural limits for `askUser` — keeps the mid-turn card usable and
/// stops the model dumping an unbounded questionnaire into one pause.
fn validate_ask_user_args(args: AskUserArgs) -> Result<AskUserArgs, String> {
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
        if opt_n < ASK_USER_MIN_OPTIONS || opt_n > ASK_USER_MAX_OPTIONS {
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
struct RawTodoArgs {
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

fn parse_todo_call(input: &str) -> Result<ToolCall, String> {
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
fn lenient_json_object<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, String> {
    let mut de = serde_json::Deserializer::from_str(input);
    serde_path_to_error::deserialize(&mut de).map_err(|e| e.to_string())
}

/// One `LlmToolDefinition` per tool `scope` allows, to advertise to the
/// model — so a customized (narrowed) allowlist only offers tools that will
/// actually succeed if called, rather than the model discovering
/// `ToolError::NotAllowed` only at execution time. Wire tag values
/// (`"readFile"`/`"listFiles"`/`"semanticSearch"`) and argument field names
/// (`path`, `startLine`+`endLine`, `depth`+`pattern`, `query`+`topK`) are
/// hand-kept in sync with `ToolCall`/`ReadFileArgs`/`ListFilesArgs`/
/// `SemanticSearchArgs` — see this module's schema round-trip test, which
/// catches drift between the two.
pub fn llm_tool_definitions(
    scope: &ToolScope,
    conversation_mode: ConversationMode,
) -> Vec<LlmToolDefinition> {
    // A tool reaches the model only if it clears *both* independent axes:
    // the project's own allowlist (`scope`, persisted, "does this project
    // permit this tool at all") and the current conversation mode
    // (`mode_tools`, per-session, "does this task-type need it right now").
    let visible = |tool: ToolName| scope.allows(tool) && mode_tools(conversation_mode).contains(&tool);
    let mut defs = Vec::new();
    if visible(ToolName::ListFiles) {
        defs.push(LlmToolDefinition {
            name: "listFiles".to_string(),
            description: "List files and directories under a path. `path` is relative to the current access-mode root: the documentation root in Docs-only mode, the repository root in Full-repo mode. Omit `path` or pass null to list that root. Use when directory structure is unknown — scaffold checks, \"what files exist here\", filename patterns. Do NOT use after `semanticSearch` already returned concrete file paths — read those with `readFile` instead. Do NOT use to explore code logic when search can locate the entry point directly. Returns an indented ASCII tree (directories end with `/`), not a flat list. The tree's first line is a display-only label for the current root (in Full-repo mode it may be the repository folder name); it is not part of any path argument. Child entries are relative to the current access-mode root. Do not manually prepend a documentation-root or repository-root segment to `path` — it is already relative to the current root. In Docs-only mode the listing includes only text documentation types (AsciiDoc, Markdown, JSON/YAML, PlantUML, Mermaid, plain text) — image binaries (.png/.svg/…) under the docs tree are intentionally omitted even when they exist on disk and are valid `image::` targets; do not treat their absence from this listing as a missing or dangling link (use check kind \"problems\" for missingImage). In Full-repo mode image files may appear; they are assets, not text to readFile."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": ["string", "null"],
                        "description": "Subdirectory relative to the current access-mode root (see tool description), or omitted/null for that root."
                    },
                    "depth": {
                        "type": ["integer", "null"],
                        "minimum": 0,
                        "description": "Maximum recursion depth below `path` (1 = only direct children, 0 = no descendant entries at all). Omit or null for no limit."
                    },
                    "pattern": {
                        "type": ["string", "null"],
                        "description": "Glob pattern (e.g. \"*.java\") matched against each entry's filename only, not its full path. Directories are always included regardless of this filter. Omit or null for no filtering."
                    }
                },
                "required": []
            }),
        });
    }
    if visible(ToolName::ReadFile) {
        defs.push(LlmToolDefinition {
            name: "readFile".to_string(),
            description: "Read the text content of one file by its path relative to the current access-mode root (documentation root in Docs-only mode, repository root in Full-repo mode), optionally restricted to a line range. Use when the relevant file is already known — especially paths from semanticSearch/grep results. For \"how does X work\" questions: after search, read at most 2–3 files first — the matching .adoc (if any) and the owning implementation (*Service / handler named by the doc or operation), not mappers, DTOs, or sibling services until the algorithm is incomplete. Prefer a line range for a large file when only part of it is relevant. Paths returned by grep/semanticSearch, and paths constructed from listFiles entries (excluding the tree's display-only root label), are already correctly rooted — pass them here as-is, with no manual prefix added or stripped."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the current access-mode root (documentation root in Docs-only mode, repository root in Full-repo mode)."
                    },
                    "startLine": {
                        "type": ["integer", "null"],
                        "minimum": 1,
                        "description": "1-indexed first line to return (inclusive). Omit or null to start from the beginning of the file."
                    },
                    "endLine": {
                        "type": ["integer", "null"],
                        "minimum": 1,
                        "description": "1-indexed last line to return (inclusive). Omit or null to read through the end of the file. Out-of-range values are clamped, not rejected."
                    }
                },
                "required": ["path"]
            }),
        });
    }
    if visible(ToolName::SemanticSearch) {
        defs.push(LlmToolDefinition {
            name: "semanticSearch".to_string(),
            description:
                "Default search tool — use this first whenever you need to find something in the project and the exact file or line is not already known. Searches via symbol lookup (exact + stem), semantic similarity, and lexical fallback. One strong first query beats several vague repeats — guess camelCase names justified by words in the question (уведомления→Notification/getNotifications, не выдумывать Patent если пользователь не сказал «патент») plus Russian business context; do not send only a lone plain word. Refine with real operation/class names only after a hit reveals them. A second call is only for a new identifier learned from readFile — prefer at most two searches per request. After results, readFile at most 2–3 entry files (adoc + owning *Service); do not listFiles the parent or open mappers/siblings until needed. If meta.hint is present, follow it on the next search. Verify with readFile before precise claims; use grep only for exhaustive exact line matches."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query: English camelCase justified by the question's own words (getNotifications from «уведомления», not invented domain prefixes) + Russian business context. Prefer identifiers over a lone plain word. Strong first query — refine only with new names from readFile."
                    },
                    "topK": {
                        "type": ["integer", "null"],
                        "minimum": 1,
                        "description": "Max number of results, default 10, capped at 50."
                    }
                },
                "required": ["query"]
            }),
        });
    }
    if visible(ToolName::Grep) {
        defs.push(LlmToolDefinition {
            name: "grep".to_string(),
            description:
                "Exact regex search over file contents under the current access-mode root (documentation root in Docs-only mode, repository root in Full-repo mode). Secondary tool — do not use as the first search step; call semanticSearch first for discovery. Use grep only when semanticSearch is insufficient: you need every call site of a symbol, every occurrence of a literal string, or a regex pattern across files, and you already know what to match. Not for conceptual or exploratory search. Returns line-oriented hits (path, 1-indexed line, line text), capped and truncated when the limit is hit. Honors .gitignore; skips binary and oversized files. Returned paths are already relative to the same root readFile uses — pass them to readFile unchanged, no prefix needed."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Rust regex pattern (no backreferences). Case-sensitive unless caseInsensitive is true."
                    },
                    "path": {
                        "type": ["string", "null"],
                        "description": "Optional subdirectory relative to the current access-mode root. Omit or null to search the whole root."
                    },
                    "glob": {
                        "type": ["string", "null"],
                        "description": "Optional filename-only glob (e.g. \"*.java\") to restrict which files are searched."
                    },
                    "caseInsensitive": {
                        "type": ["boolean", "null"],
                        "description": "When true, match case-insensitively. Default false."
                    },
                    "maxResults": {
                        "type": ["integer", "null"],
                        "minimum": 1,
                        "description": "Max number of line hits to return, default 50, capped at 200."
                    }
                },
                "required": ["pattern"]
            }),
        });
    }
    if visible(ToolName::GitDiff) {
        defs.push(LlmToolDefinition {
            name: "gitDiff".to_string(),
            description:
                "Show the git diff for one file — recent local changes (unstaged working-tree vs index/HEAD, or staged index vs HEAD) or the change introduced by a specific commit. Path is relative to the current access-mode root (documentation root in Docs-only mode, repository root in Full-repo mode). Use this to reason about what changed recently, not just the current file content. Combine with readFile to understand both current state and history. Returns a unified diff (truncated for large changes) plus +/- line counts."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the current access-mode root."
                    },
                    "scope": {
                        "type": ["string", "null"],
                        "enum": ["unstaged", "staged", null],
                        "description": "Working-tree scope: \"unstaged\" (default) or \"staged\". Ignored when `commit` is set."
                    },
                    "commit": {
                        "type": ["string", "null"],
                        "description": "Optional commit hash/ref. When set, returns the parent→commit file diff and ignores `scope`."
                    }
                },
                "required": ["path"]
            }),
        });
    }
    if visible(ToolName::GitBlame) {
        defs.push(LlmToolDefinition {
            name: "gitBlame".to_string(),
            description:
                "Show line authorship (git blame) for one file as contiguous hunks sharing the same commit — who last changed which lines, when, and the commit summary. Path is relative to the current access-mode root. Optionally restrict to a 1-indexed inclusive line range; large ranges are capped. Use this to understand the history behind specific lines, not just their current content — investigate when a particular piece of content was introduced, or trace the origin of a decision or implementation detail. Combine with readFile to understand both current state and history."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the current access-mode root."
                    },
                    "startLine": {
                        "type": ["integer", "null"],
                        "minimum": 1,
                        "description": "1-indexed first line (inclusive). Omit or null to start from line 1."
                    },
                    "endLine": {
                        "type": ["integer", "null"],
                        "minimum": 1,
                        "description": "1-indexed last line (inclusive). Omit or null to continue through the file (still subject to the per-call line cap)."
                    }
                },
                "required": ["path"]
            }),
        });
    }
    if visible(ToolName::Check) {
        defs.push(LlmToolDefinition {
            name: "check".to_string(),
            description:
                "Run a documentation verification and return findings. Two kinds are available:\n\nkind \"problems\": the same list the editor's Problems panel shows — broken xref/include/image targets, missing anchors, duplicate anchors, circular includes, and parse errors. Checks cover ONLY supported indexed documentation file types under the documentation root (.adoc/.asciidoc, .md/.markdown, .json, .yaml/.yml, .txt, .puml/.plantuml, .mmd/.mermaid) — not arbitrary repository source code or unsupported extensions. Recomputes diagnostics before returning so results are fresh. Use this to verify documentation integrity before and after edits.\n\nkind \"standards\": checks API-method documentation folders against the corporate documentation standard (К.1.1–К.7.1, weighted criteria, 80% pass threshold per method folder). Purely local file reads, no network access — link-correctness (К.1.3) is out of scope. Use this to audit API documentation quality.\n\nOptional `path` and every finding's `document` field use the current access-mode root (same as readFile/writeFile) — pass them between tools unchanged."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["problems", "standards"],
                        "description": "Which verification to run. \"problems\" = workspace diagnostics (Problems panel) for integrity checks. \"standards\" = API-documentation corporate standard compliance (К.1.1–К.7.1, weighted, 80% pass threshold per method folder)."
                    },
                    "path": {
                        "type": ["string", "null"],
                        "description": "Optional path relative to the current access-mode root (same as readFile); must resolve under the documentation tree. For \"problems\": a single file to check (omit/null to check all indexed documentation files). For \"standards\": a method folder to check, or any file inside one (its parent folder is used) — omit/null to check the entire documentation tree."
                    }
                },
                "required": ["kind"]
            }),
        });
    }
    if visible(ToolName::WriteFile) {
        defs.push(LlmToolDefinition {
            name: "writeFile".to_string(),
            description:
                "Create or overwrite one documentation file's full content, given its path relative to the current access-mode root (same as readFile/listFiles). The path must resolve under the documentation tree — paths outside it are rejected with an error. Any missing parent directories in the path are created automatically — there is no need to call createDirectory first. Always requires explicit user approval before the write actually happens — the user may deny it, in which case the file is left unchanged. Do not retry automatically after a denial; ask the user how they'd like to proceed instead. Only recognized documentation file types can be written."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the current access-mode root (documentation root in Docs-only, repository root in Full-repo — same as readFile). Must resolve under the documentation tree; paths outside it are rejected. Must be a recognized documentation file type."
                    },
                    "content": {
                        "type": "string",
                        "description": "The full new content of the file, replacing any existing content."
                    }
                },
                "required": ["path", "content"]
            }),
        });
    }
    if visible(ToolName::EditFile) {
        defs.push(LlmToolDefinition {
            name: "editFile".to_string(),
            description:
                "Make one or more precise, targeted edits to an existing documentation file by replacing exact snippets of its current content, given its path relative to the current access-mode root (same as readFile/listFiles). The path must resolve under the documentation tree — paths outside it are rejected with an error. Each edit's `old` text should match the file's CURRENT content exactly once, and all edits in one call are validated against the file's original content and applied together, or none are — they are independent of each other and of their order (atomic application). If an edit's `old` doesn't match exactly (whitespace/formatting drift, or you're recalling the content from memory rather than a fresh read), the call may be rejected; some sessions may attempt automatic reconciliation, but treat exact matching as the contract and add a few more surrounding lines to `old` to make it unique and exact. Prefer this over writeFile for small, localized changes: it's cheaper and safer than resending the whole file. Always requires explicit user approval before anything is written."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the current access-mode root (same as readFile). Must resolve under the documentation tree; paths outside it are rejected. Must be a recognized documentation file type and must already exist."
                    },
                    "edits": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "old": {
                                    "type": "string",
                                    "description": "Exact text to find — must appear exactly once in the file's current content."
                                },
                                "new": {
                                    "type": "string",
                                    "description": "Text to replace it with."
                                }
                            },
                            "required": ["old", "new"]
                        },
                        "description": "One or more find-and-replace edits, applied together against the file's original content (atomic), not sequentially against each other's output."
                    }
                },
                "required": ["path", "edits"]
            }),
        });
    }
    if visible(ToolName::DeleteFile) {
        defs.push(LlmToolDefinition {
            name: "deleteFile".to_string(),
            description:
                "Delete one file, given its path relative to the current access-mode root (same as readFile/listFiles). The path must resolve under the documentation tree — paths outside it are rejected with an error. This is irreversible — do not call it speculatively. Always requires explicit user approval before the deletion actually happens — the user may deny it, in which case the file is left unchanged."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the current access-mode root (same as readFile). Must resolve under the documentation tree; paths outside it are rejected."
                    }
                },
                "required": ["path"]
            }),
        });
    }
    if visible(ToolName::CreateDirectory) {
        defs.push(LlmToolDefinition {
            name: "createDirectory".to_string(),
            description:
                "Create a directory (including any missing parent directories) given its path relative to the current access-mode root (same as readFile/listFiles). The path must resolve under the documentation tree — paths outside it are rejected with an error. Use this only when the directory itself must exist (for example, an empty folder or a template scaffold). Do not call it solely to prepare parent directories for writeFile — writeFile creates missing parent directories automatically. For a new REST API method's documentation folder, pass template: \"restEndpoint\" — the same scaffold the editor's \"New folder\" dialog offers: `{methodName}.adoc`, `request.adoc`, `response.adoc`, and `{methodName}.puml`, where `{methodName}` is the final path segment. The `request.adoc`/`response.adoc` names are always bare, never prefixed with the method name — one folder is one method by convention, so the prefix would be redundant; do not rename them to match differently-named legacy folders. Omit template (or pass null) for an empty directory. The call is subject to the project's approval settings; if approval is required and the user denies it, nothing is created. Fails if the path already exists as a file or directory, so a successful result always means the folder is newly created, never pre-existing. On success, the result's `createdFiles` field already lists every generated file's exact path — treat it as authoritative and do not call listFiles to re-verify what was created."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path relative to the current access-mode root (same as readFile). Must resolve under the documentation tree. For template: \"restEndpoint\", the final path segment is used as the method name for generated filenames."
                    },
                    "template": {
                        "type": ["string", "null"],
                        "enum": ["restEndpoint", null],
                        "description": "Optional folder scaffold. \"restEndpoint\" creates REST-method documentation files inside the new folder (same as the UI template \"Документация на REST метод\"). Omit or null for an empty directory."
                    }
                },
                "required": ["path"]
            }),
        });
    }
    if visible(ToolName::DeleteDirectory) {
        defs.push(LlmToolDefinition {
            name: "deleteDirectory".to_string(),
            description:
                "Delete a directory, given its path relative to the current access-mode root (same as readFile/listFiles). The path must resolve under the documentation tree — paths outside it are rejected with an error. By default (recursive omitted or false), the call is rejected if the directory is not empty — delete its contents first, or pass recursive: true to delete the directory and everything inside it in one call. This is irreversible, especially with recursive: true — do not call it speculatively. Always requires explicit user approval before the deletion actually happens — the user may deny it."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path relative to the current access-mode root (same as readFile). Must resolve under the documentation tree; paths outside it are rejected."
                    },
                    "recursive": {
                        "type": ["boolean", "null"],
                        "description": "If true, deletes the directory and all its contents. If omitted or false (default), the call is rejected when the directory is not empty."
                    }
                },
                "required": ["path"]
            }),
        });
    }
    if visible(ToolName::Move) {
        defs.push(LlmToolDefinition {
            name: "move".to_string(),
            description:
                "Move or rename a file or directory, given its current path and a new path, both relative to the current access-mode root (same as readFile). Both must resolve under the documentation tree — paths outside it are rejected with an error. This is one operation covering both cases: a newPath in the same directory with a different name is a rename, a newPath elsewhere is a move (optionally with a new name too). Works for both files and directories — there is no separate rename tool or directory-specific variant. References to the old path elsewhere in the documentation (include::, xref:, and JSON/YAML $ref) are updated automatically so they keep pointing at the right file. Fails if something already exists at newPath — nothing is overwritten. newPath's parent directory must already exist — use createDirectory first if it doesn't. Unlike writeFile, move does not create missing parent directories. Always requires explicit user approval before anything changes — the user may deny it."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Current path relative to the current access-mode root (same as readFile). Must resolve under the documentation tree."
                    },
                    "newPath": {
                        "type": "string",
                        "description": "New path relative to the current access-mode root (same as readFile). Must resolve under the documentation tree. Fails if something already exists there."
                    }
                },
                "required": ["path", "newPath"]
            }),
        });
    }
    if visible(ToolName::RequestFullRepoAccess) {
        defs.push(LlmToolDefinition {
            name: "requestFullRepoAccess".to_string(),
            description:
                "Request escalating from docs-only to full-repo access when repository access beyond documentation is genuinely needed to answer the user's request. Requires a stated reason, and always requires explicit user approval — the user may deny it. Do not call this speculatively or repeatedly; only when docs-only access is clearly insufficient."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "reason": {
                        "type": "string",
                        "description": "Why full-repo access is needed for the current request."
                    }
                },
                "required": ["reason"]
            }),
        });
    }
    if visible(ToolName::Todo) {
        defs.push(LlmToolDefinition {
            name: "todo".to_string(),
            description: "Manage your working task checklist for a multi-step request (3+ steps). One tool, two operations selected via `op`. `op: \"write\"` adds new task titles (`tasks`, an array of short imperative strings, 3-7 words each) to the end of the checklist; the runtime assigns each an id and, if the checklist was empty before this call, marks the first of the new tasks in_progress automatically (the rest start pending) — calling `write` again later appends more titles to the end, it never replaces or clears the existing list. `op: \"update\"` changes one existing task, named by `id` exactly as shown in your current checklist, to `status: \"completed\"` or `status: \"cancelled\"` (optionally with a short `note`: a brief result for a completed task, or the reason for a cancelled one) — these are the ONLY two status values you may set; you can never set `pending` or `in_progress` yourself, the runtime handles those transitions automatically, including auto-activating the next pending task the instant the current one is completed or cancelled. There is no `read` operation: your current checklist, with the active task marked, is always shown to you at the top of your context — never call this tool just to see the list. Do not use this tool for a task with only 1-2 steps, that is a wasted call. At most 20 tasks total in one checklist."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "op": {
                        "type": "string",
                        "enum": ["write", "update"],
                        "description": "\"write\" to add new tasks (uses `tasks`). \"update\" to change one existing task's status/note (uses `id`, `status`, optionally `note`)."
                    },
                    "tasks": {
                        "type": ["array", "null"],
                        "items": { "type": "string" },
                        "description": "Only for op: \"write\". New task titles to append to the end of the checklist, each 3-7 words, imperative. Ignored for op: \"update\"."
                    },
                    "id": {
                        "type": ["string", "null"],
                        "description": "Only for op: \"update\". The id of the task to change (e.g. \"t2\"), exactly as shown in your current checklist. Ignored for op: \"write\"."
                    },
                    "status": {
                        "type": ["string", "null"],
                        "enum": ["completed", "cancelled", null],
                        "description": "Only for op: \"update\". The task's new status. Only \"completed\" or \"cancelled\" are valid — pending/in_progress are runtime-managed and cannot be set here. Use \"cancelled\" when a task turns out unnecessary or impossible, with `note` explaining why. Ignored for op: \"write\"."
                    },
                    "note": {
                        "type": ["string", "null"],
                        "description": "Only for op: \"update\". Optional short note: a brief result for a completed task, or the reason for a cancelled one. Ignored for op: \"write\"."
                    }
                },
                "required": ["op"]
            }),
        });
    }
    if visible(ToolName::RequestModeSwitch) {
        defs.push(LlmToolDefinition {
            name: "requestModeSwitch".to_string(),
            description:
                "Request switching the conversation to a different mode (\"agent\", \"plan\", or \"question\") when the current mode structurally cannot do what the user is asking. In Plan mode, when asked to actually implement/apply something: request \"agent\". In Question mode, when a request needs a multi-step plan: request \"plan\"; when it needs actual file changes: request \"agent\". In Agent mode, when the request is really just a question with no changes needed: request \"question\"; when it clearly needs a plan drafted first: request \"plan\". Requires a stated reason, and always requires explicit user approval — the user may deny it. Do not call this speculatively; only when the current mode is genuinely the wrong fit for the request. An approved switch does not change the toolset mid-turn: the new mode applies starting with the next user message — after approval, confirm briefly and stop; do not attempt tools that only the new mode would allow."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["agent", "plan", "question"],
                        "description": "The mode being requested."
                    },
                    "reason": {
                        "type": "string",
                        "description": "Why the current mode doesn't fit the current request."
                    }
                },
                "required": ["mode", "reason"]
            }),
        });
    }
    if visible(ToolName::GetAsciidocTemplates) {
        defs.push(LlmToolDefinition {
            name: "getAsciidocTemplates".to_string(),
            description: format!(
                "Fetch the full canonical AsciiDoc markup for one or more house element templates (tables, admonitions, lists, includes, etc.) by id, from this fixed catalog:\n\n{}\nCall this before drafting a table, admonition block, list, or include that matches one of the entries above, passing its `id` (multiple ids may be requested in one call). Reuse the returned markup as the baseline for what you write — only placeholder values/content change — instead of inventing different syntax. If none of the entries fit the specific need, plain AsciiDoc without calling this is fine.",
                asciidoc_template_catalog_description()
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "description": "One or more template ids from the catalog above."
                    }
                },
                "required": ["ids"]
            }),
        });
    }
    if visible(ToolName::Skill) {
        defs.push(LlmToolDefinition {
            name: "skill".to_string(),
            description:
                "Search and load specialized instruction packs (skills) on demand. Do not guess skill names and do not expect a catalog in this description. First call with op \"search\" and a short query about the current task (required — empty query is rejected). Then op \"load\" with a matching name to get full instructions. If those instructions point to a companion file, op \"read\" with name and path. Use this before filling a REST method folder after its scaffold, or when working with OpenAPI specs layout (schemas/operations/$ref) or any user-installed pack. Ordinary AsciiDoc authoring does not need a skill."
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
        });
    }
    if visible(ToolName::AskUser) {
        defs.push(LlmToolDefinition {
            name: "askUser".to_string(),
            description:
                "Ask the user one or more structured clarifying questions mid-turn and wait for their answers before continuing. Use when you genuinely cannot proceed without a choice (blocking fork, conflicting requirements, equally valid alternatives). Do NOT use for rhetorical questions, anything already visible in the repo, or when a reasonable default can be chosen and briefly mentioned. Prefer calling this alone in its own tool round — do not bundle with write/edit/delete. Do not also write the same question as plain chat text in the same turn. Keep 1–4 questions; options should be concrete and mutually exclusive unless allowMultiple is true. The UI always offers a free-text field — the user may pick options, type their own answer, or both. Treat `customText` in the tool result as the user's real intent when present. Available in every conversation mode (agent, plan, question)."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": ["string", "null"],
                        "description": "Optional short card title shown above the questions."
                    },
                    "questions": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 4,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {
                                    "type": "string",
                                    "description": "Stable id for this question (returned in the answer payload)."
                                },
                                "prompt": {
                                    "type": "string",
                                    "description": "The question text shown to the user."
                                },
                                "options": {
                                    "type": "array",
                                    "minItems": 2,
                                    "maxItems": 6,
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "id": { "type": "string" },
                                            "label": { "type": "string" }
                                        },
                                        "required": ["id", "label"]
                                    }
                                },
                                "allowMultiple": {
                                    "type": "boolean",
                                    "description": "If true, the user may select more than one option (checkboxes). Default false (radio)."
                                }
                            },
                            "required": ["id", "prompt", "options"]
                        }
                    }
                },
                "required": ["questions"]
            }),
        });
    }
    if visible(ToolName::CreatePlan) {
        defs.push(LlmToolDefinition {
            name: "createPlan".to_string(),
            description:
                "Create a persisted work plan as the final deliverable of Plan mode. Call this AFTER research with read-only tools — do not dump the full plan as chat prose; the UI shows a plan card from this tool result. `name` is a short 3–4 word title; `overview` is 1–2 sentences; `plan` is the full markdown body (first line MUST be a `# Title` heading); `todos` is an array of at least 2 concrete checklist items with stable slug `id`s (e.g. \"setup-auth\") and imperative `content`. Returns `planId` — remember it for later `updatePlan` calls in this session. After success, reply with a brief 1–3 sentence summary only; the card has «Открыть» / «Начать» buttons."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Short plan title, 3–4 words."
                    },
                    "overview": {
                        "type": "string",
                        "description": "1–2 sentence summary of the goal."
                    },
                    "plan": {
                        "type": "string",
                        "description": "Full markdown plan body; first line must be `# Title`."
                    },
                    "todos": {
                        "type": "array",
                        "minItems": 2,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {
                                    "type": "string",
                                    "description": "Stable slug id (e.g. \"update-controller\")."
                                },
                                "content": {
                                    "type": "string",
                                    "description": "Imperative step description."
                                }
                            },
                            "required": ["id", "content"]
                        },
                        "description": "Checklist of concrete implementation steps (min 2)."
                    }
                },
                "required": ["name", "overview", "plan", "todos"]
            }),
        });
    }
    if visible(ToolName::UpdatePlan) {
        defs.push(LlmToolDefinition {
            name: "updatePlan".to_string(),
            description:
                "Update an existing plan created earlier in this Plan-mode session (same `planId` from `createPlan`). Pass only the fields that change. When replacing `todos`, supply the full new checklist (min 2 items) — statuses reset. Do not create a second plan for refinements."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "planId": {
                        "type": "string",
                        "description": "Id returned by createPlan."
                    },
                    "name": {
                        "type": ["string", "null"],
                        "description": "Optional new short title."
                    },
                    "overview": {
                        "type": ["string", "null"],
                        "description": "Optional new overview."
                    },
                    "plan": {
                        "type": ["string", "null"],
                        "description": "Optional new full markdown body."
                    },
                    "todos": {
                        "type": ["array", "null"],
                        "minItems": 2,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "content": { "type": "string" }
                            },
                            "required": ["id", "content"]
                        },
                        "description": "Optional full replacement checklist."
                    }
                },
                "required": ["planId"]
            }),
        });
    }
    if visible(ToolName::ReadPlan) {
        defs.push(LlmToolDefinition {
            name: "readPlan".to_string(),
            description:
                "Load a persisted plan by `planId` — full markdown body and current todo statuses. Use in Agent mode before executing an approved plan, or in Plan mode to refresh context before `updatePlan`."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "planId": {
                        "type": "string",
                        "description": "Plan id to load."
                    }
                },
                "required": ["planId"]
            }),
        });
    }
    if visible(ToolName::UpdatePlanTodo) {
        defs.push(LlmToolDefinition {
            name: "updatePlanTodo".to_string(),
            description:
                "Mark one step of a persisted plan as `completed` or `cancelled` while executing it in Agent mode. Runtime auto-promotes the next pending step to in_progress. Use the todo `id` from `readPlan` / `createPlan` exactly. Optional `note` for a brief result or cancellation reason."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "planId": {
                        "type": "string",
                        "description": "Plan id."
                    },
                    "id": {
                        "type": "string",
                        "description": "Todo id within that plan."
                    },
                    "status": {
                        "type": "string",
                        "enum": ["completed", "cancelled"],
                        "description": "New status — only completed or cancelled."
                    },
                    "note": {
                        "type": ["string", "null"],
                        "description": "Optional short note."
                    }
                },
                "required": ["planId", "id", "status"]
            }),
        });
    }
    defs
}

/// Renders `ASCIIDOC_ELEMENT_TEMPLATES` as a compact, grouped index for
/// `getAsciidocTemplates`'s own tool description — generated straight from
/// the same catalog `get_asciidoc_templates` looks ids up in, so the index
/// the model sees can never list an id the tool can't actually resolve (or
/// vice versa). Relies on the catalog already being grouped by category in
/// declaration order (structure, tables, examples, includes).
fn asciidoc_template_catalog_description() -> String {
    fn category_label(category: &str) -> &str {
        match category {
            "structure" => "Структура",
            "tables" => "Таблицы",
            "examples" => "Примеры",
            "includes" => "Вставки",
            other => other,
        }
    }
    let mut out = String::new();
    let mut current_category = "";
    for t in ASCIIDOC_ELEMENT_TEMPLATES {
        if t.category != current_category {
            out.push_str(category_label(t.category));
            out.push_str(":\n");
            current_category = t.category;
        }
        out.push_str(&format!("- `{}`: {} — {}\n", t.id, t.label, t.description));
    }
    out
}

fn list_files(scope: &ToolScope, args: ListFilesArgs) -> Result<Vec<ToolFileEntry>, ToolError> {
    let subdir = resolve_subdir(scope, args.path.as_deref())?;

    let mut entries = match scope.mode {
        AiAccessMode::DocsOnly => list_docs_only(scope, subdir.as_ref(), args.depth)?,
        AiAccessMode::FullRepo => {
            list_full_repo(scope, subdir.map(|(_, abs)| abs), args.depth)?
        }
    };

    if let Some(pattern) = args.pattern.as_deref() {
        let matcher = compile_glob(pattern)?;
        // Directories are always kept — `pattern` scopes which *files*
        // come back, not the navigable structure. This applies in `FullRepo`
        // mode too: `list_full_repo` reports real directory entries (see
        // `workspace_scanner::scan_all_entries_with_depth`), so a pattern
        // like "*.java" no longer hides the directories those files live in.
        entries.retain(|e| e.is_dir || matcher.is_match(basename(&e.path)));
    }

    Ok(entries)
}

/// `ToolFileEntry::path` is always `/`-separated by construction
/// (`paths::relative_to`), so a plain `rsplit` avoids any
/// `std::path::Path`/OsStr platform quirks.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn compile_glob(pattern: &str) -> Result<globset::GlobMatcher, ToolError> {
    globset::Glob::new(pattern)
        .map(|g| g.compile_matcher())
        .map_err(|e| ToolError::InvalidPattern(e.to_string()))
}

/// Exact regex content search under `scope.root` — delegates to
/// `services::docs_search::search_under_root` (shared with the user-facing
/// `docs_search` IPC). Paths in results are scope-root-relative so they
/// round-trip into `readFile`.
fn grep(scope: &ToolScope, args: GrepArgs) -> Result<ToolResult, ToolError> {
    let payload = docs_search::search_under_root(&scope.root, &args)?;
    Ok(ToolResult::GrepResults {
        matches: payload.matches,
        truncated: payload.truncated,
    })
}

/// One directory level of the tree `render_file_tree` builds out of a flat
/// `ToolFileEntry` list — children sorted by name (`BTreeMap`) so the
/// rendered tree is deterministic regardless of the scan order the entries
/// arrived in. `is_file` is only meaningful on a leaf with no children of
/// its own; an intermediate path segment (inferred from some deeper entry's
/// path, never listed directly) is always rendered as a directory.
#[derive(Default)]
struct TreeBuildNode {
    children: std::collections::BTreeMap<String, TreeBuildNode>,
    is_file: bool,
}

/// Renders a flat `listFiles` result as an indented ASCII tree (à la `tree(1)`)
/// instead of a JSON array — so the model can see the whole directory
/// structure and where each file sits at a glance, rather than reconstructing
/// it from N separate `path` strings. The first line is always `./` (the
/// access-mode root), never the on-disk folder name, so a docs-root folder
/// such as `asciidoc` is not mistaken for a child to prepend onto paths.
pub fn render_file_tree(entries: &[ToolFileEntry]) -> String {
    let mut root = TreeBuildNode::default();
    for entry in entries {
        let mut node = &mut root;
        let parts: Vec<&str> = entry.path.split('/').filter(|p| !p.is_empty()).collect();
        let Some((last, dirs)) = parts.split_last() else {
            continue;
        };
        for part in dirs {
            node = node.children.entry((*part).to_string()).or_default();
        }
        let leaf = node.children.entry((*last).to_string()).or_default();
        leaf.is_file = !entry.is_dir;
    }

    let mut out = String::from("./\n");
    render_tree_children(&root, "", &mut out);
    out
}

fn render_tree_children(node: &TreeBuildNode, prefix: &str, out: &mut String) {
    let count = node.children.len();
    for (i, (name, child)) in node.children.iter().enumerate() {
        let is_last = i + 1 == count;
        let is_dir = !child.children.is_empty() || !child.is_file;
        out.push_str(prefix);
        out.push_str(if is_last { "└── " } else { "├── " });
        out.push_str(name);
        if is_dir {
            out.push('/');
        }
        out.push('\n');
        if is_dir {
            let child_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
            render_tree_children(child, &child_prefix, out);
        }
    }
}

/// One `readFile` result: a possibly-partial slice of a file's lines,
/// along with enough range/total metadata for the model to know it's
/// looking at less than the whole file.
struct FileSlice {
    content: String,
    start_line: u32,
    end_line: u32,
    total_lines: u32,
}

fn read_file(scope: &ToolScope, args: ReadFileArgs) -> Result<FileSlice, ToolError> {
    // No extension filtering here, unlike `docs_fs::read_project_file` —
    // the tool boundary is containment under `scope.root` alone. In
    // `FullRepo` mode the harness must be able to read source files, which
    // aren't in `is_supported_file`'s doc-format list.
    let joined = paths::join_relative(&scope.root, &args.path)?;
    let canonical = paths::ensure_under(&scope.root, &joined)?;
    if !canonical.exists() {
        return Err(ToolError::NotFound(args.path));
    }
    if !canonical.is_file() {
        return Err(ToolError::NotAFile(args.path));
    }
    let content = fs::read_to_string(&canonical).map_err(ToolError::Io)?;
    Ok(slice_lines(content, args.start_line, args.end_line))
}

/// Relativize `absolute` against a root when `absolute` may not exist yet
/// (write/create destinations). Prefer strip_prefix against a canonicalized
/// root — `ensure_under` already produced a path under that root — falling
/// back to `relative_to_lenient` when needed.
fn relative_under_maybe_missing(root: &Path, absolute: &Path) -> Result<String, ToolError> {
    let root_canon = root
        .canonicalize()
        .map_err(|e| ToolError::Io(e))?;
    if absolute == root_canon.as_path() {
        return Ok(".".to_string());
    }
    if let Ok(rel) = absolute.strip_prefix(&root_canon) {
        let mut parts = Vec::new();
        for component in rel.components() {
            match component {
                std::path::Component::Normal(s) => {
                    parts.push(s.to_string_lossy().into_owned());
                }
                std::path::Component::CurDir => {}
                _ => {
                    return Err(ToolError::PathEscape(absolute.display().to_string()));
                }
            }
        }
        return Ok(parts.join("/"));
    }
    Ok(paths::relative_to_lenient(root, absolute)?.replace('\\', "/"))
}

/// Resolve a mutate/`check` path against the access-mode root, then require
/// it under `docs_root`. Returns `(access_relative, docs_relative)`.
/// `docs_relative` is computed by subtracting the known docs root after
/// containment — not by stripping a prefix from the raw model argument.
pub fn resolve_mutable_docs_path(
    scope: &ToolScope,
    path: &str,
) -> Result<(String, String), ToolError> {
    let joined = paths::join_relative(&scope.root, path)?;
    let under_root = paths::ensure_under(&scope.root, &joined)?;
    let under_docs = match paths::ensure_under(&scope.docs_root, &under_root) {
        Ok(p) => p,
        Err(ProjectError::PathEscape(_)) => {
            return Err(ToolError::OutsideDocumentation(path.to_string()));
        }
        Err(e) => return Err(e.into()),
    };
    let access_rel = relative_under_maybe_missing(&scope.root, &under_docs)?;
    let docs_rel = relative_under_maybe_missing(&scope.docs_root, &under_docs)?;
    let access_rel = if access_rel == "." {
        String::new()
    } else {
        access_rel
    };
    let docs_rel = if docs_rel == "." {
        String::new()
    } else {
        docs_rel
    };
    Ok((access_rel, docs_rel))
}

/// Convert a repo-relative path (index/`FileId`/`DocumentId` space) into the
/// access-mode-relative path the model should see.
pub fn to_access_relative(scope: &ToolScope, repo_relative: &str) -> Option<String> {
    if repo_relative.is_empty() || repo_relative == "." {
        return Some(String::new());
    }
    let abs = scope.repo_root.join(repo_relative);
    let under_root = paths::ensure_under(&scope.root, &abs).ok()?;
    let rel = relative_under_maybe_missing(&scope.root, &under_root).ok()?;
    Some(if rel == "." { String::new() } else { rel })
}

/// Docs-root-relative → access-mode-relative (for scaffold/move side-effect
/// paths that are already docs-relative internally).
fn docs_rel_to_access_rel(scope: &ToolScope, docs_rel: &str) -> String {
    if scope.root == scope.docs_root {
        return docs_rel.to_string();
    }
    if docs_rel.is_empty() || docs_rel == "." {
        // Access-relative path of the docs root itself under the repo.
        return relative_under_maybe_missing(&scope.root, &scope.docs_root)
            .unwrap_or_default();
    }
    let abs = scope.docs_root.join(docs_rel);
    relative_under_maybe_missing(&scope.root, &abs).unwrap_or_else(|_| docs_rel.to_string())
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

/// Resolve a scope-root-relative tool path to a repo-relative path safe for
/// `git2`, after the same `ensure_under(scope.root)` gate every other
/// read tool uses — this is what keeps `gitDiff`/`gitBlame` safe in
/// DocsOnly (they cannot read tracked blobs outside `docsRoot`).
fn resolve_repo_relative_path(scope: &ToolScope, path: &str) -> Result<String, ToolError> {
    let joined = paths::join_relative(&scope.root, path)?;
    let canonical = paths::ensure_under(&scope.root, &joined)?;
    // Don't require the path to exist on disk — a staged-delete or
    // commit-only path may not be in the worktree, but git still knows it.
    // Containment under `scope.root` is enough.
    let rel = paths::relative_to_lenient(&scope.repo_root, &canonical)?;
    Ok(rel.replace('\\', "/"))
}

fn git_diff(scope: &ToolScope, args: GitDiffArgs) -> Result<ToolResult, ToolError> {
    let repo_rel = resolve_repo_relative_path(scope, &args.path)?;
    let repo_root = scope.repo_root.to_string_lossy();

    let file_diff = if let Some(commit) = args.commit.as_deref().filter(|c| !c.is_empty()) {
        git_ops::commit_file_diff(&repo_root, commit, &repo_rel)?
    } else {
        let diff_scope = match args.scope.as_deref() {
            None | Some("unstaged") => GitDiffScope::Unstaged,
            Some("staged") => GitDiffScope::Staged,
            Some(other) => {
                return Err(ToolError::InvalidArguments {
                    tool: "gitDiff".into(),
                    reason: format!(
                        "scope must be \"unstaged\" or \"staged\" (got \"{other}\")"
                    ),
                });
            }
        };
        git_ops::file_diff(&repo_root, &repo_rel, diff_scope)?
    };

    let label = format!("{} → {}", file_diff.original_label, file_diff.modified_label);
    let diff = if file_diff.is_binary {
        FileDiffStats {
            lines_added: 0,
            lines_removed: 0,
            unified_diff: String::new(),
            truncated: false,
        }
    } else {
        text_diff::diff_stats(&file_diff.original, &file_diff.modified)
    };

    Ok(ToolResult::GitDiff {
        path: args.path,
        label,
        diff,
        is_binary: file_diff.is_binary,
    })
}

fn git_blame(scope: &ToolScope, args: GitBlameArgs) -> Result<ToolResult, ToolError> {
    let joined = paths::join_relative(&scope.root, &args.path)?;
    let canonical = paths::ensure_under(&scope.root, &joined)?;
    let repo_rel = paths::relative_to_lenient(&scope.repo_root, &canonical)?.replace('\\', "/");
    let repo_root = scope.repo_root.to_string_lossy();

    let start = args.start_line.unwrap_or(1).max(1);
    let file_lines = fs::read_to_string(&canonical)
        .map(|s| s.lines().count() as u32)
        .unwrap_or(0);

    let (end, truncated) = match args.end_line {
        Some(e) => {
            let e = e.max(start);
            if e - start + 1 > MAX_BLAME_LINES {
                (start + MAX_BLAME_LINES - 1, true)
            } else {
                (e, false)
            }
        }
        None => {
            let capped = start + MAX_BLAME_LINES - 1;
            if file_lines == 0 {
                (capped, false)
            } else if file_lines > capped {
                (capped, true)
            } else {
                (file_lines.max(start), false)
            }
        }
    };

    let hunks = git_ops::blame(&repo_root, &repo_rel, Some(start), Some(end))?;
    Ok(ToolResult::GitBlame {
        path: args.path,
        hunks,
        truncated,
    })
}

/// Recomputes workspace diagnostics then returns them — same findings as
/// BottomDock «Проблемы» / standards — path args use the access-mode root;
/// must still resolve under `docs_root`. Result paths are rewritten to
/// access-mode-relative before returning to the model.
fn check(
    scope: &ToolScope,
    args: CheckArgs,
    deps: &EmbeddingDeps,
) -> Result<ToolResult, ToolError> {
    match args.kind {
        CheckKind::Problems => check_problems(scope, args.path.as_deref(), deps),
        CheckKind::Standards => check_doc_standards(scope, args.path.as_deref()),
    }
}

fn check_problems(
    scope: &ToolScope,
    path: Option<&str>,
    deps: &EmbeddingDeps,
) -> Result<ToolResult, ToolError> {
    let mut diagnostics = match path {
        None => {
            diagnostics::run_all(&deps.workspace_index);
            deps.workspace_index.get_diagnostics()
        }
        Some(access_path) => {
            let doc_id = access_path_to_document_id(scope, access_path)?;
            diagnostics::run_for(&deps.workspace_index, &doc_id);
            deps.workspace_index.get_diagnostics_for(&doc_id)
        }
    };

    let truncated = diagnostics.len() > MAX_CHECK_DIAGNOSTICS;
    if truncated {
        diagnostics.truncate(MAX_CHECK_DIAGNOSTICS);
    }

    for d in &mut diagnostics {
        if let Some(access) = to_access_relative(scope, &d.document.0) {
            let old = d.document.0.clone();
            if old != access {
                d.message = d.message.replace(&old, &access);
            }
            d.document = DocumentId::new(access);
        }
    }

    Ok(ToolResult::CheckResults {
        kind: CheckKind::Problems,
        diagnostics,
        truncated,
    })
}

/// Access-mode-relative path → repo-relative `DocumentId`, after requiring
/// containment under `scope.docs_root`.
fn access_path_to_document_id(scope: &ToolScope, access_path: &str) -> Result<DocumentId, ToolError> {
    let (_access_rel, docs_rel) = resolve_mutable_docs_path(scope, access_path)?;
    let docs_root = scope.docs_root.to_string_lossy();
    let suffix = reference_rewrite::docs_root_suffix(&scope.repo_root, &docs_root).ok_or_else(
        || ToolError::InvalidArguments {
            tool: "check".into(),
            reason: "documentation root is not under the repository root".into(),
        },
    )?;
    Ok(DocumentId::new(reference_rewrite::to_repo_relative(
        &suffix, &docs_rel,
    )))
}

/// Runs the API-documentation standards checker (`services::standards`,
/// same engine as the «Стандарты» panel's «Проверить» button) against the
/// whole docs root, then — when `path` narrows it — filters down to just
/// one method folder. Always walks the full docs root first rather than
/// rooting `check_repository` at the target folder directly: that
/// function's own root argument doubles as "the container, not itself a
/// checkable folder" (mirrors the real docs root never being a method
/// folder), so pointing it straight at a method folder would incorrectly
/// skip evaluating that very folder. This mirrors how the «Стандарты»
/// panel's "Текущий файл" tab scopes too (client-side filtering over a
/// full-project report). Unlike `check_problems`'s `path` (a single file),
/// `standards` checking operates at directory granularity — per К.1.1 of
/// the standard, a "unit" is a `methodName` folder, not a file — so a file
/// path is resolved to its parent directory rather than rejected.
fn check_doc_standards(scope: &ToolScope, path: Option<&str>) -> Result<ToolResult, ToolError> {
    let config = standards_prefs::load_standards_config().unwrap_or_default();
    let mut report = standards::check_repository(&scope.docs_root, &config);

    if let Some(access_path) = path {
        let (_access_rel, docs_rel) = resolve_mutable_docs_path(scope, access_path)?;
        let joined = paths::join_relative(&scope.docs_root, &docs_rel)?;
        let canonical = paths::ensure_under(&scope.docs_root, &joined)?;
        let target_dir = if canonical.is_file() {
            canonical.parent().map(Path::to_path_buf).unwrap_or(canonical)
        } else {
            canonical
        };
        let target_rel =
            paths::relative_to(&scope.docs_root, &target_dir).unwrap_or_default();
        let prefix = format!("{target_rel}/");
        report.folders.retain(|f| f.folder == target_rel || f.folder.starts_with(&prefix));
        report.overall_passed =
            !report.folders.is_empty() && report.folders.iter().all(|f| f.passed);
    }

    // Failing folders first, so a truncated response still surfaces what's
    // most worth the model's attention.
    report.folders.sort_by_key(|f| f.passed);
    let truncated = report.folders.len() > MAX_STANDARDS_FOLDERS;
    if truncated {
        report.folders.truncate(MAX_STANDARDS_FOLDERS);
    }

    for folder in &mut report.folders {
        folder.folder = docs_rel_to_access_rel(scope, &folder.folder);
    }

    Ok(ToolResult::StandardsChecked { report, truncated })
}

/// Clamps `start_line`/`end_line` into range rather than erroring (mirrors
/// `SemanticSearchArgs.top_k`'s `.clamp(1, MAX_TOP_K)` handling below).
/// When neither is requested, `content` is returned byte-identical to what
/// `fs::read_to_string` produced — no split/rejoin round trip for the
/// common full-file case. An empty file reports `start_line: 0,
/// end_line: 0, total_lines: 0` (there is no line 1 to claim). If
/// `end_line` clamps below `start_line` after each is independently
/// clamped into `[1, total_lines]`, `end_line` is raised to `start_line`
/// (returns that one line) rather than erroring.
fn slice_lines(content: String, start_line: Option<u32>, end_line: Option<u32>) -> FileSlice {
    if start_line.is_none() && end_line.is_none() {
        let total_lines = content.lines().count() as u32;
        let start_line = if total_lines == 0 { 0 } else { 1 };
        return FileSlice { content, start_line, end_line: total_lines, total_lines };
    }
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len() as u32;
    if total_lines == 0 {
        return FileSlice { content: String::new(), start_line: 0, end_line: 0, total_lines: 0 };
    }
    let start = start_line.unwrap_or(1).clamp(1, total_lines);
    let end = end_line.unwrap_or(total_lines).clamp(start, total_lines);
    let mut sliced = lines[(start - 1) as usize..end as usize].join("\n");
    sliced.push('\n');
    FileSlice { content: sliced, start_line: start, end_line: end, total_lines }
}

/// Resolves the path against the access-mode root, then requires it under
/// `docs_root` — Full-repo widens what the assistant can *read*, not what
/// it may write. Reuses `docs_fs::write_project_file` with the docs-relative
/// path: create-or-overwrite, creates parent directories, rejects unsupported
/// extensions.
fn write_file(
    scope: &ToolScope,
    args: WriteFileArgs,
    deps: &EmbeddingDeps,
) -> Result<(String, FileDiffStats), ToolError> {
    let (access_rel, docs_rel) = resolve_mutable_docs_path(scope, &args.path)?;
    reject_atlas_memory_path(scope, &docs_rel)?;
    let docs_root = scope.docs_root.to_string_lossy();
    // NotFound-tolerant read, same pattern as `commands::project::
    // read_project_file_or_none` — a brand-new file diffs against `""`
    // rather than needing its own special case.
    let old = match docs_fs::read_project_file(&docs_root, &docs_rel) {
        Ok(content) => content,
        Err(ProjectError::NotFound(_)) => String::new(),
        Err(e) => return Err(e.into()),
    };
    docs_fs::write_project_file(&docs_root, &docs_rel, &args.content)?;
    // Best-effort: keeps the in-memory index in step with this write
    // immediately, rather than only once the async file-watcher gets to it
    // — which otherwise regularly still lags behind by the time the next
    // tool call in the same round (e.g. `move`'s reference lookup, or
    // `check`) reads the index. Never fails the call itself — a write that
    // succeeded on disk must not be reported as failed just because the
    // index update lagged (e.g. `EmbeddingDeps::empty()` in tests, or no
    // project open).
    let _ = deps.workspace_index.update_document(scope.docs_root.join(&docs_rel));
    let diff = text_diff::diff_stats(&old, &args.content);
    Ok((access_rel, diff))
}

/// Same docs-root containment as `write_file`, but composes
/// `docs_fs::read_project_file` + `apply_edits` + `docs_fs::
/// write_project_file` instead of taking new content directly — the file
/// must already exist (a missing file surfaces `read_project_file`'s own
/// `NotFound`, converted via `ToolError`'s `From<ProjectError>`); creating
/// new files stays `write_file`'s job. `fast_apply` is `edit_file`'s own
/// `EmbeddingDeps::fast_apply` field, threaded straight through to
/// `apply_edits`.
fn edit_file(
    scope: &ToolScope,
    args: EditFileArgs,
    fast_apply: Option<&(Arc<dyn LlmProvider>, String)>,
    deps: &EmbeddingDeps,
) -> Result<(String, FileDiffStats), ToolError> {
    let (access_rel, docs_rel) = resolve_mutable_docs_path(scope, &args.path)?;
    reject_atlas_memory_path(scope, &docs_rel)?;
    let docs_root = scope.docs_root.to_string_lossy();
    let content = docs_fs::read_project_file(&docs_root, &docs_rel)?;
    let edited = apply_edits(&content, &args.edits, fast_apply)?;
    docs_fs::write_project_file(&docs_root, &docs_rel, &edited)?;
    // See `write_file`'s matching comment — same best-effort sync.
    let _ = deps.workspace_index.update_document(scope.docs_root.join(&docs_rel));
    let diff = text_diff::diff_stats(&content, &edited);
    Ok((access_rel, diff))
}

/// Applies every edit in `edits` to `content`. The primary path is exact and
/// all-or-nothing, unchanged from before this module had a fast-apply
/// fallback: each edit's `old` is looked up in `content` *as given* — never
/// against the output of an earlier edit in the same call — so edits are
/// independent of each other and of their own order; `old` missing entirely
/// (`EditTextNotFound`), appearing more than once (`EditTextAmbiguous`), or
/// two edits' matched regions overlapping (`EditsOverlap`) all reject the
/// whole call with nothing written, exactly as documented on those
/// `ToolError` variants.
///
/// If that exact pass fails with a per-edit matching problem
/// (`EditTextNotFound`/`EditTextAmbiguous` — *not* `EditsOverlap`, which is a
/// problem with the call itself no amount of reconciliation fixes) and
/// `fast_apply` is `Some`, this falls back to
/// `apply_edits_sequential_with_fallback` instead of failing outright — see
/// its doc comment for how that differs (sequential, not all-at-once). If
/// the fallback itself can't produce a safe result either, its own
/// `EditApplyFailed` (which explains *why* reconciliation failed) is
/// surfaced rather than the plain exact-match error — strictly more useful
/// to a model deciding how to retry, since it confirms reconciliation was
/// attempted at all.
fn apply_edits(
    content: &str,
    edits: &[FileEdit],
    fast_apply: Option<&(Arc<dyn LlmProvider>, String)>,
) -> Result<String, ToolError> {
    match apply_edits_exact(content, edits) {
        Ok(result) => Ok(result),
        Err(ToolError::EditTextNotFound(_) | ToolError::EditTextAmbiguous(_, _))
            if fast_apply.is_some() =>
        {
            apply_edits_sequential_with_fallback(content, edits, fast_apply.unwrap())
        }
        Err(err) => Err(err),
    }
}

/// The exact, all-or-nothing matching pass — see `apply_edits`'s doc
/// comment. A pure function with no dependency on `fast_apply`, kept
/// separate so it stays simple to test and reason about on its own.
fn apply_edits_exact(content: &str, edits: &[FileEdit]) -> Result<String, ToolError> {
    let ranges = exact_match_ranges(content, edits)?;

    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;
    for (start, end, new) in ranges {
        result.push_str(&content[cursor..start]);
        result.push_str(new);
        cursor = end;
    }
    result.push_str(&content[cursor..]);
    Ok(result)
}

/// Resolves every edit's `old` to a unique `(start, end)` byte range in
/// `content`, sorted by position, after checking none of them overlap.
/// Shared by `apply_edits_exact` (which splices all of them into `content`
/// at once) and `find_unique_exact_match` (which needs just one edit's
/// range, reusing the same not-found/ambiguous checks).
fn exact_match_ranges<'a>(
    content: &str,
    edits: &'a [FileEdit],
) -> Result<Vec<(usize, usize, &'a str)>, ToolError> {
    let mut ranges: Vec<(usize, usize, &str)> = Vec::with_capacity(edits.len());
    for edit in edits {
        let (start, end) = find_unique_exact_match(content, &edit.old)?;
        ranges.push((start, end, edit.new.as_str()));
    }

    ranges.sort_by_key(|&(start, _, _)| start);
    for pair in ranges.windows(2) {
        let (_, prev_end, _) = pair[0];
        let (next_start, _, _) = pair[1];
        if next_start < prev_end {
            return Err(ToolError::EditsOverlap);
        }
    }
    Ok(ranges)
}

/// Looks up `old`'s single occurrence in `content` — the same check
/// `apply_edits_exact` runs per edit, extracted so
/// `apply_edits_sequential_with_fallback` can run it one edit at a time
/// against its own, possibly already-modified, view of the content.
fn find_unique_exact_match(content: &str, old: &str) -> Result<(usize, usize), ToolError> {
    let mut occurrences = content.match_indices(old);
    let Some((start, _)) = occurrences.next() else {
        return Err(ToolError::EditTextNotFound(old.to_string()));
    };
    let count = 1 + occurrences.count();
    if count > 1 {
        return Err(ToolError::EditTextAmbiguous(old.to_string(), count));
    }
    Ok((start, start + old.len()))
}

/// The fast-apply fallback path: unlike `apply_edits_exact`, edits here are
/// applied one at a time, each against the result of the previous one — not
/// independently against the original content — because a fast-apply call
/// has no fixed byte range to validate for overlap against the others; it
/// can only sensibly operate on "the file as it stands right now". For each
/// edit, an exact match is still tried first (free, deterministic, and
/// correctness-preserving); only an edit that still doesn't match exactly
/// against its current view of the content is escalated to
/// `run_fast_apply`. A model that sent a batch where every edit happens to
/// match exactly sees identical output to `apply_edits_exact`, just reached
/// less directly.
fn apply_edits_sequential_with_fallback(
    content: &str,
    edits: &[FileEdit],
    fast_apply: &(Arc<dyn LlmProvider>, String),
) -> Result<String, ToolError> {
    let mut current = content.to_string();
    for edit in edits {
        current = match find_unique_exact_match(&current, &edit.old) {
            Ok((start, end)) => {
                let mut spliced = String::with_capacity(current.len());
                spliced.push_str(&current[..start]);
                spliced.push_str(&edit.new);
                spliced.push_str(&current[end..]);
                spliced
            }
            Err(_) => run_fast_apply(fast_apply, &current, edit)
                .map_err(|reason| ToolError::EditApplyFailed(truncate_snippet(&edit.old), reason))?,
        };
    }
    Ok(current)
}

/// A file larger than this is not sent through fast-apply — the whole
/// content round-trips through the model's context twice over (once in the
/// request, once in the response), so an arbitrarily large document isn't
/// worth the token cost/latency this would add; deterministic matching (or
/// `writeFile` for a rewrite that size) is the right tool past this size.
/// ~40k characters is generous for the documentation files this tool
/// targets (a few thousand lines of prose/markup).
const FAST_APPLY_MAX_CONTENT_CHARS: usize = 40_000;

const FAST_APPLY_SYSTEM_PROMPT: &str = "You are a precise text-patching engine. You will be given the full current text of a document and one intended edit, expressed as an approximate `old` snippet (it may not match the document's exact current whitespace, line breaks, or formatting) and its `new` replacement. Find the location in the document that the `old` snippet is describing and apply the edit there. Output ONLY the complete resulting document text: every part of the document outside the edited region must be byte-for-byte identical to the input. Do not add any commentary, explanation, or markdown code fences — output the raw document text and nothing else.";

/// Sends `content` plus one edit's intent to the fast-apply model and
/// returns its reconciled full-file output, or an `Err` reason string (never
/// a `ToolError` directly — the caller, `apply_edits_sequential_with_fallback`,
/// wraps it with the edit's own `old` text via `ToolError::EditApplyFailed`).
/// The model's raw output is defensively unwrapped from a markdown code
/// fence if present (`strip_code_fence`) before being checked by
/// `validate_fast_apply_output` — nothing this function returns is ever
/// trusted without that check passing.
fn run_fast_apply(
    fast_apply: &(Arc<dyn LlmProvider>, String),
    content: &str,
    edit: &FileEdit,
) -> Result<String, String> {
    if content.chars().count() > FAST_APPLY_MAX_CONTENT_CHARS {
        return Err("file is too large for automatic reconciliation".to_string());
    }
    let (provider, model) = fast_apply;
    let request = ChatRequest {
        model: model.clone(),
        tools: Vec::new(),
        messages: vec![
            LlmMessage {
                role: LlmRole::System,
                content: Some(FAST_APPLY_SYSTEM_PROMPT.to_string()),
                tool_call_id: None,
                tool_calls: vec![],
            },
            LlmMessage {
                role: LlmRole::User,
                content: Some(format!(
                    "FILE CONTENT:\n```\n{content}\n```\n\nREPLACE THIS TEXT:\n```\n{old}\n```\n\nWITH THIS TEXT:\n```\n{new}\n```\n\nOutput the complete updated file content only.",
                    content = content,
                    old = edit.old,
                    new = edit.new,
                )),
                tool_call_id: None,
                tool_calls: vec![],
            },
        ],
    };
    let response = provider.chat(request).map_err(|e| format!("provider error: {e}"))?;
    let raw = response.content.ok_or_else(|| "model returned no content".to_string())?;
    let candidate = strip_code_fence(&raw);
    validate_fast_apply_output(content, &candidate, edit)?;
    Ok(candidate)
}

/// Best-effort removal of a single wrapping markdown code fence — despite
/// `FAST_APPLY_SYSTEM_PROMPT` explicitly asking for raw output, models
/// reliably wrap it in ` ```…``` ` (optionally with a language tag on the
/// opening fence) out of habit. Only strips a fence that wraps the *entire*
/// response (first line is a fence, last line is a fence, at least one line
/// of content between them) — leaves anything else untouched rather than
/// mangling a response that never had a wrapping fence in the first place.
fn strip_code_fence(text: &str) -> String {
    let trimmed = text.trim();
    let mut lines: Vec<&str> = trimmed.lines().collect();
    if lines.len() >= 2
        && lines[0].trim_start().starts_with("```")
        && lines[lines.len() - 1].trim() == "```"
    {
        lines.pop();
        lines.remove(0);
        return lines.join("\n");
    }
    text.to_string()
}

/// The fast-apply safety net: `candidate` is only accepted if the model
/// changed nothing outside a bounded region around the intended edit. Finds
/// the longest common prefix and (non-overlapping) longest common suffix
/// between `original` and `candidate`; the unmatched middle on each side is
/// what the model actually changed. Rejects if either middle is implausibly
/// large for the edit that was requested (more than 3x the larger of
/// `old`/`new`'s length, plus a fixed slack for minor reformatting) — a
/// generous bound for a legitimate single-region edit, but well short of
/// what a model rewriting unrelated parts of the file would produce. Also
/// rejects output identical to the input (the model reported success
/// without actually changing anything) or empty output for non-empty input.
fn validate_fast_apply_output(original: &str, candidate: &str, edit: &FileEdit) -> Result<(), String> {
    if candidate.is_empty() && !original.is_empty() {
        return Err("model returned an empty file".to_string());
    }
    if candidate == original {
        return Err("model made no change".to_string());
    }

    let original_bytes = original.as_bytes();
    let candidate_bytes = candidate.as_bytes();
    let max_common = original_bytes.len().min(candidate_bytes.len());

    let prefix_len = original_bytes
        .iter()
        .zip(candidate_bytes.iter())
        .take(max_common)
        .take_while(|(a, b)| a == b)
        .count();

    let max_suffix = max_common - prefix_len;
    let suffix_len = original_bytes[prefix_len..]
        .iter()
        .rev()
        .zip(candidate_bytes[prefix_len..].iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();

    let original_middle = original_bytes.len() - prefix_len - suffix_len;
    let candidate_middle = candidate_bytes.len() - prefix_len - suffix_len;

    let cap = edit.old.len().max(edit.new.len()) * 3 + 200;
    if original_middle > cap || candidate_middle > cap {
        return Err(
            "model changed more of the file than the requested edit accounts for — rejected for safety"
                .to_string(),
        );
    }
    Ok(())
}

/// Same docs-root containment as `write_file`. Reuses
/// `docs_fs::delete_project_file`: fails if the path is missing or not a file.
fn delete_file(
    scope: &ToolScope,
    args: DeleteFileArgs,
    deps: &EmbeddingDeps,
) -> Result<(String, FileDiffStats), ToolError> {
    let (access_rel, docs_rel) = resolve_mutable_docs_path(scope, &args.path)?;
    reject_atlas_memory_path(scope, &docs_rel)?;
    let docs_root = scope.docs_root.to_string_lossy();
    // Missing-file already errors below via `delete_project_file`'s own
    // `NotFound` (see `delete_file_rejects_missing_file`) — this read just
    // surfaces that same error slightly earlier, with no fallback needed.
    let old = docs_fs::read_project_file(&docs_root, &docs_rel)?;
    docs_fs::delete_project_file(&docs_root, &docs_rel)?;
    // See `write_file`'s matching comment — same best-effort sync, so a
    // `check` (or another tool) called right after doesn't still see
    // diagnostics for a file that's already gone.
    let _ = deps.workspace_index.remove_document(scope.docs_root.join(&docs_rel));
    let diff = text_diff::diff_stats(&old, "");
    Ok((access_rel, diff))
}

/// Same docs-root containment as `write_file`. Without a template, reuses
/// `docs_fs::create_project_dir` (creates missing parents, fails if the path
/// already exists). With `template: "restEndpoint"`, reuses
/// `docs_fs::create_rest_endpoint_folder`. Returns `(access_path, template,
/// created_files)` with access-mode-relative paths.
fn create_directory(
    scope: &ToolScope,
    args: CreateDirectoryArgs,
    deps: &EmbeddingDeps,
) -> Result<(String, Option<String>, Vec<String>), ToolError> {
    let (access_rel, docs_rel) = resolve_mutable_docs_path(scope, &args.path)?;
    reject_atlas_memory_path(scope, &docs_rel)?;
    let docs_root = scope.docs_root.to_string_lossy();
    match args.template.as_deref() {
        None => {
            docs_fs::create_project_dir(&docs_root, &docs_rel)?;
            Ok((access_rel, None, Vec::new()))
        }
        Some("restEndpoint") => {
            let method_name = basename(&docs_rel);
            if method_name.is_empty() || method_name == "." {
                return Err(ToolError::InvalidArguments {
                    tool: "createDirectory".into(),
                    reason: "path must end with a folder name to use as the REST method name"
                        .into(),
                });
            }
            let created_docs = rest_endpoint_created_files(&docs_rel, method_name);
            docs_fs::create_rest_endpoint_folder(&docs_root, &docs_rel, method_name)?;
            // See `write_file`'s matching comment — same best-effort sync,
            // for every file the scaffold just created.
            for file in &created_docs {
                let _ = deps.workspace_index.update_document(scope.docs_root.join(file));
            }
            let created_files: Vec<String> = created_docs
                .into_iter()
                .map(|p| docs_rel_to_access_rel(scope, &p))
                .collect();
            Ok((access_rel, Some("restEndpoint".into()), created_files))
        }
        Some(other) => Err(ToolError::InvalidArguments {
            tool: "createDirectory".into(),
            reason: format!(
                "unknown template \"{other}\" (expected \"restEndpoint\" or null)"
            ),
        }),
    }
}

/// Docs-relative paths `create_rest_endpoint_folder` writes for `method_name`
/// under `folder_path` — kept in sync with `docs_fs::create_rest_endpoint_folder`.
fn rest_endpoint_created_files(folder_path: &str, method_name: &str) -> Vec<String> {
    let child = |name: &str| -> String {
        if folder_path.is_empty() || folder_path == "." {
            name.to_string()
        } else {
            format!("{folder_path}/{name}")
        }
    };
    vec![
        child(&format!("{method_name}.adoc")),
        child("request.adoc"),
        child("response.adoc"),
        child(&format!("{method_name}.puml")),
    ]
}

/// Same docs-root containment as `write_file`.
/// `recursive` defaults to `false` when omitted (`Option::unwrap_or`) —
/// `docs_fs::delete_project_dir` then refuses a non-empty directory with
/// `ToolError::DirectoryNotEmpty` rather than silently deleting its
/// contents; pass `recursive: true` to delete a non-empty directory in one
/// call.
fn delete_directory(
    scope: &ToolScope,
    args: DeleteDirectoryArgs,
    deps: &EmbeddingDeps,
) -> Result<String, ToolError> {
    let (access_rel, docs_rel) = resolve_mutable_docs_path(scope, &args.path)?;
    reject_atlas_memory_path(scope, &docs_rel)?;
    let recursive = args.recursive.unwrap_or(false);
    // Only a `recursive` delete can remove indexed files at all — a
    // non-recursive delete only ever succeeds against an already-empty
    // directory, which by definition holds nothing the index tracks.
    // Enumerated and cleared from the index *before* the actual removal
    // (mirrors `move_path`'s "read the index before mutating the
    // filesystem" ordering): `remove_document` resolves each file's index
    // id by walking up from its path, which breaks once a parent
    // directory is gone — as every parent below the top one would be,
    // partway through removing a multi-level subtree.
    if recursive {
        let abs_dir = scope.docs_root.join(&docs_rel);
        if let Ok(nested) = crate::infra::workspace_scanner::scan_all(&abs_dir) {
            for file in nested {
                let _ = deps.workspace_index.remove_document(file.path);
            }
        }
    }
    docs_fs::delete_project_dir(&scope.docs_root.to_string_lossy(), &docs_rel, recursive)?;
    Ok(access_rel)
}

/// Covers both moving and renaming, both files and directories — path args
/// use the access-mode root, then must resolve under `docs_root`. Picks
/// `docs_fs::rename_project_file` vs `rename_project_dir` via a cheap,
/// non-canonicalized `is_dir()` probe. Returns `(from, to, updated_files)`
/// with access-mode-relative paths (including `updated_files` entries).
fn move_path(
    scope: &ToolScope,
    args: MoveArgs,
    deps: &EmbeddingDeps,
) -> Result<(String, String, Vec<UpdatedReference>), ToolError> {
    let (access_from, docs_from) = resolve_mutable_docs_path(scope, &args.path)?;
    let (access_to, docs_to) = resolve_mutable_docs_path(scope, &args.new_path)?;
    reject_atlas_memory_path(scope, &docs_from)?;
    reject_atlas_memory_path(scope, &docs_to)?;
    let docs_root = scope.docs_root.to_string_lossy();
    let is_dir = scope.docs_root.join(&docs_from).is_dir();

    // Also captures `renamed` (the repo-relative old/new pairs) so it's
    // still available below, after the actual filesystem move, to update
    // the moved document(s)' own index rows — empty when `docs_root`
    // doesn't resolve under `repo_root`, same "skip the cascade entirely"
    // case `updated_files` already handles.
    let mut renamed: Vec<reference_rewrite::RenamedPath> = Vec::new();
    let mut updated_files =
        match reference_rewrite::docs_root_suffix(&scope.repo_root, &docs_root) {
            Some(suffix) => {
                let old = reference_rewrite::to_repo_relative(&suffix, &docs_from);
                let new = reference_rewrite::to_repo_relative(&suffix, &docs_to);
                renamed = if is_dir {
                    reference_rewrite::renamed_paths_for_dir_move(&deps.workspace_index, &old, &new)
                } else {
                    vec![reference_rewrite::RenamedPath { old, new }]
                };
                let rewritten = reference_rewrite::rewrite_references(
                    &deps.workspace_index,
                    &scope.repo_root,
                    &renamed,
                )
                .map_err(|e| ToolError::Io(std::io::Error::other(e.to_string())))?;
                reference_rewrite::into_report(&suffix, rewritten).updated_files
            }
            None => Vec::new(),
        };

    if is_dir {
        docs_fs::rename_project_dir(&docs_root, &docs_from, &docs_to)?;
    } else {
        docs_fs::rename_project_file(&docs_root, &docs_from, &docs_to)?;
    }

    // See `write_file`'s matching comment — same best-effort sync, so the
    // moved document(s) themselves are found at their new path immediately
    // (their content still gets re-parsed asynchronously, same as any
    // other `update_document`/`index_file` call, but their existence and
    // path are correct right away — including for a `move` chained
    // straight after this one in the same round).
    for pair in &renamed {
        let _ = deps
            .workspace_index
            .rename_document(scope.repo_root.join(&pair.old), scope.repo_root.join(&pair.new));
    }

    for u in &mut updated_files {
        u.docs_relative_path = docs_rel_to_access_rel(scope, &u.docs_relative_path);
    }

    Ok((access_from, access_to, updated_files))
}

/// Hard-deny mutate tools against `{repo}/.atlas/memory/**` — the OptMem
/// store is managed only by the `memory` tool. Prompt text alone is not
/// enough when `docsRoot` is the repo root (`.txt` is a supported docs
/// extension). `relative` is docs-root-relative (after
/// `resolve_mutable_docs_path`).
fn reject_atlas_memory_path(scope: &ToolScope, relative: &str) -> Result<(), ToolError> {
    let joined = paths::join_relative(&scope.docs_root, relative)?;
    if agent_memory::path_is_under_project_memory(&scope.repo_root, &joined) {
        return Err(ToolError::PathEscape(format!(
            "protected agent memory store (.atlas/memory): {relative}"
        )));
    }
    Ok(())
}

/// Validates an optional subdirectory argument once, shared by both mode
/// branches: returns its root-relative string form (for the docs-only
/// prefix filter) and its canonical absolute form (for the full-repo scan
/// root).
fn resolve_subdir(
    scope: &ToolScope,
    path: Option<&str>,
) -> Result<Option<(String, PathBuf)>, ToolError> {
    let Some(path) = path else {
        return Ok(None);
    };
    if path.is_empty() || path == "." {
        return Ok(None);
    }
    let joined = paths::join_relative(&scope.root, path)?;
    let canonical = paths::ensure_under(&scope.root, &joined)?;
    if !canonical.is_dir() {
        return Err(ToolError::NotFound(path.to_string()));
    }
    let rel = paths::relative_to(&scope.root, &canonical)?;
    Ok(Some((rel, canonical)))
}

fn list_docs_only(
    scope: &ToolScope,
    subdir: Option<&(String, PathBuf)>,
    max_depth: Option<u32>,
) -> Result<Vec<ToolFileEntry>, ToolError> {
    let dir = subdir.map(|(_, abs)| abs.as_path()).unwrap_or(scope.root.as_path());
    let tree = docs_fs::list_docs_tree_scoped(&scope.root, dir, max_depth)?;
    let mut entries = Vec::new();
    flatten_tree(tree, &mut entries);
    Ok(entries)
}

fn flatten_tree(nodes: Vec<TreeNode>, out: &mut Vec<ToolFileEntry>) {
    for node in nodes {
        out.push(ToolFileEntry {
            path: node.path,
            is_dir: node.is_dir,
        });
        if let Some(children) = node.children {
            flatten_tree(children, out);
        }
    }
}

fn list_full_repo(
    scope: &ToolScope,
    scan_root: Option<PathBuf>,
    max_depth: Option<u32>,
) -> Result<Vec<ToolFileEntry>, ToolError> {
    let scan_root = scan_root.unwrap_or_else(|| scope.root.clone());
    let entries =
        workspace_scanner::scan_all_entries_with_depth(&scan_root, max_depth.map(|d| d as usize))?;
    entries
        .into_iter()
        .map(|e| {
            let rel = paths::relative_to(&scope.root, &e.path)?;
            Ok(ToolFileEntry {
                path: rel,
                is_dir: e.is_dir,
            })
        })
        .collect()
}

/// Cascade entry point: an exact symbol-name hit (cheapest, always tried)
/// is prepended to whichever of the semantic/lexical tiers fills the
/// remaining `top_k` budget, chosen by `is_semantic_ready`. Returns matches
/// plus `meta` (extracted tokens, weak-search hint) for the model/UI.
fn semantic_search(
    scope: &ToolScope,
    args: SemanticSearchArgs,
    deps: &EmbeddingDeps,
) -> Result<SemanticSearchPayload, ToolError> {
    let top_k = args.top_k.unwrap_or(DEFAULT_TOP_K).clamp(1, MAX_TOP_K);
    let extracted_tokens = extract_search_tokens(&args.query);

    // Exact-name / path-segment tier stays authoritative/unboosted — it's
    // already the cheapest, most-precise signal, not the "did you mean
    // something in the same file family" heuristic `related` below is.
    let mut results = symbol_matches(&deps.repo_index, scope, &args.query, top_k);

    let mut tiers_used = vec!["symbol".to_string()];
    let remaining = top_k.saturating_sub(results.len());
    if remaining > 0 {
        let related = deps
            .active_file
            .as_ref()
            .map(|file_id| related_files(deps, file_id))
            .unwrap_or_default();

        // Over-fetch when a boost could reorder results — a related-but-not-
        // quite-top-ranked chunk needs candidates beyond `remaining` to have any
        // chance of surfacing after boosting.
        let fetch_k = if related.is_empty() {
            remaining
        } else {
            (remaining * 3).min(MAX_TOP_K * 3)
        };

        let tier_results = if is_semantic_ready(deps) {
            tiers_used.push("semantic".to_string());
            semantic_matches(scope, deps, &args.query, fetch_k)?
        } else {
            tiers_used.push("lexical".to_string());
            lexical_matches(&deps.chunk_index, scope, &args.query, fetch_k)
        };

        results.extend(apply_related_boost(tier_results, &related, remaining));
    }

    let symbol_hits = results
        .iter()
        .filter(|m| m.source == MatchSource::Symbol)
        .count() as u32;
    let has_semantic = results.iter().any(|m| m.source == MatchSource::Semantic);
    let only_lexical = !results.is_empty()
        && results.iter().all(|m| m.source == MatchSource::Lexical)
        && symbol_hits == 0;
    let (weak, hint) = weak_search_hint(SearchMetaInput {
        match_count: results.len(),
        symbol_hits,
        has_semantic,
        only_lexical,
        tiers_used: &tiers_used,
        extracted_tokens: &extracted_tokens,
    });

    Ok(SemanticSearchPayload {
        matches: results,
        meta: SemanticSearchMeta {
            tiers_used,
            symbol_hits,
            extracted_tokens,
            weak,
            hint,
        },
    })
}

/// Boosts (multiplicatively, via `RELATED_FILE_BOOST`) every match whose
/// file is in `related`, re-sorts descending by the (possibly boosted)
/// score, then truncates to `budget` — the final step of the cascade's
/// semantic/lexical tier, pulled out as its own pure function so it's
/// testable without going through `is_semantic_ready`'s project-state
/// lookups. A no-op re-sort when `related` is empty would still be correct
/// but is skipped as a cheap early-out, matching `semantic_search`'s
/// pre-existing behavior for "no active file".
fn apply_related_boost(
    mut matches: Vec<ToolMatch>,
    related: &HashSet<FileId>,
    budget: usize,
) -> Vec<ToolMatch> {
    if !related.is_empty() {
        for m in &mut matches {
            if related.contains(&FileId(m.path.clone())) {
                m.score *= RELATED_FILE_BOOST;
            }
        }
        matches.sort_by(|a, b| b.score.total_cmp(&a.score));
    }
    matches.truncate(budget);
    matches
}

/// A light nudge (not a hard filter) applied to a search result's score when
/// its file is one `related_files` returns for the currently-open editor
/// tab — multiplicative so it scales sensibly against both the semantic
/// tier's cosine-similarity scores (roughly `0..1`) and the lexical
/// fallback's raw occurrence counts, without needing a tier-specific
/// constant.
const RELATED_FILE_BOOST: f32 = 1.25;

/// Combines both dependency graphs `RepositoryIndex`/`WorkspaceIndex`
/// already compute for `file_id`, one hop, forward-only: Java imports (via
/// `RepositoryIndex::java_dependencies`) and AsciiDoc/JSON/YAML
/// includes+`$ref`s (via `WorkspaceIndex::find_includes`/`find_references`).
/// Same combination `commands::embeddings.rs`'s first-sync priority code
/// already performs (`direct_dependencies` + `java_dependencies`), kept as
/// its own small helper here rather than imported — `services` must not
/// depend on `commands`.
fn related_files(deps: &EmbeddingDeps, file_id: &FileId) -> HashSet<FileId> {
    let mut out: HashSet<FileId> = deps.repo_index.java_dependencies(file_id).into_iter().collect();

    let doc_id = crate::domain::workspace_index::DocumentId::new(file_id.0.clone());
    for inc in deps.workspace_index.find_includes(&doc_id) {
        out.insert(FileId(inc.path));
    }
    for r in deps.workspace_index.find_references(&doc_id) {
        if !r.target_document.is_empty() {
            out.insert(FileId(r.target_document));
        }
    }
    out
}

/// Mirrors `commands::embeddings::embedding_index_status`'s readiness check
/// exactly (`resolve_index_paths` -> `attach_index_store` -> stale check ->
/// `attach_embedding_index(allow_repair: false)` -> `embedded_count > 0`),
/// plus a `try_lock` peek at `EmbeddingSyncGuard` for "a sync is actively
/// running right now". The guard is never held through this check or the
/// search that follows — its `try_lock` guard value is dropped immediately
/// (never bound to a variable), matching `embedding_index_status`'s own
/// precedent of never acquiring this guard at all for a read. Any failure
/// along the rest of this sequence (no project open, a transient
/// store-open error) degrades to "not ready" rather than propagating —
/// consistent with the whole feature being a graceful cascade, not a
/// pipeline that should hard-fail just because the fast path had a hiccup.
fn is_semantic_ready(deps: &EmbeddingDeps) -> bool {
    // `WouldBlock` (a sync is actively running right now) is the only
    // `try_lock` outcome that should degrade this call — `Poisoned` must
    // not, or a single panic elsewhere while holding this guard (see
    // `commands::embeddings::lock_sync_guard`'s doc comment) would disable
    // semantic search for the rest of the app's lifetime instead of just
    // this one call.
    if matches!(deps.sync_guard.try_lock(), Err(TryLockError::WouldBlock)) {
        return false;
    }

    let Ok(Some(project)) = project_open::get_project() else {
        return false;
    };
    let Ok((index_root, storage_dir)) = resolve_index_paths(&project) else {
        return false;
    };
    let Ok((store, stale)) =
        attach_index_store(&deps.chunk_index, &deps.index_store, &storage_dir, &index_root)
    else {
        return false;
    };
    if stale {
        return false;
    }

    let Ok(config) = embedding_config::resolve_embedding_config() else {
        return false;
    };
    let dimensions = embedding_providers::expected_dimensions(&config);
    if attach_embedding_index(&deps.embedding_index, &store, &index_root, dimensions, false).is_err()
    {
        return false;
    }

    let Ok(slot) = deps.embedding_index.lock() else {
        return false;
    };
    slot.as_ref().is_some_and(|(_, _, index)| index.len() > 0)
}

/// Embeds `query`, searches the resident `EmbeddingIndex`, and resolves
/// each hit's chunk text. Independently re-derives `index_root`/attaches
/// the store/index rather than reusing `is_semantic_ready`'s work — cheap
/// and idempotent (each attach short-circuits when already current),
/// matching how `embedding_sync`/`embedding_index_status` each separately
/// re-resolve this instead of sharing state across calls.
fn semantic_matches(
    scope: &ToolScope,
    deps: &EmbeddingDeps,
    query: &str,
    top_k: usize,
) -> Result<Vec<ToolMatch>, ToolError> {
    let project = project_open::get_project()
        .map_err(|e| ToolError::SemanticSearch(e.to_string()))?
        .ok_or_else(|| ToolError::SemanticSearch("no project is open".to_string()))?;
    let (index_root, storage_dir) =
        resolve_index_paths(&project).map_err(ToolError::SemanticSearch)?;
    let (store, _stale) = attach_index_store(
        &deps.chunk_index,
        &deps.index_store,
        &storage_dir,
        &index_root,
    )
    .map_err(ToolError::SemanticSearch)?;

    let config = embedding_config::resolve_embedding_config()
        .map_err(|e| ToolError::SemanticSearch(e.to_string()))?;
    let dimensions = embedding_providers::expected_dimensions(&config);
    attach_embedding_index(&deps.embedding_index, &store, &index_root, dimensions, false)
        .map_err(ToolError::SemanticSearch)?;

    let api_key = embedding_credentials_store::get_api_key();
    let provider = ensure_provider(&deps.embedding_provider, &config, api_key)
        .map_err(ToolError::SemanticSearch)?;

    let query_embedding = provider
        .embed(&[query])?
        .into_iter()
        .next()
        .ok_or_else(|| {
            ToolError::SemanticSearch("embedding provider returned no vector".to_string())
        })?;

    let hits = {
        let slot = deps.embedding_index.lock().map_err(|_| {
            ToolError::SemanticSearch("embedding index lock poisoned".to_string())
        })?;
        let Some((_, _, index)) = slot.as_ref() else {
            return Ok(Vec::new());
        };
        // This `usearch` wrapper has no predicate-aware ANN search — when
        // this scope filters results (`DocsOnly`), over-fetch the whole
        // corpus so filtering below can still fill `top_k` from whatever's
        // left, rather than silently returning fewer hits than the caller
        // asked for just because the nearest raw neighbors happened to be
        // outside `docs_root`.
        let search_k = if scope.mode == AiAccessMode::DocsOnly {
            top_k.max(index.len())
        } else {
            top_k
        };
        index.search(&query_embedding, search_k)?
    };

    let mut out = Vec::with_capacity(top_k.min(hits.len()));
    for (chunk_id, distance) in hits {
        if out.len() >= top_k {
            break;
        }
        let Some(metadata) = deps.chunk_index.get(&chunk_id) else {
            continue;
        };
        if !scope.allows_search_result(&metadata.file_id) {
            continue;
        }
        let Ok(text) = resolve_text(&scope.repo_root, &metadata) else {
            continue;
        };
        let Some(access_path) = to_access_relative(scope, &metadata.file_id.0) else {
            continue;
        };
        out.push(ToolMatch {
            path: access_path,
            snippet: truncate_snippet(&text),
            // `EmbeddingIndex::search` returns cosine distance (lower is
            // closer) — flip to a "higher is better" similarity score.
            score: 1.0 - distance,
            start_byte: metadata.start_byte,
            end_byte: metadata.end_byte,
            qualified_name: metadata.qualified_name,
            source: MatchSource::Semantic,
        });
    }
    Ok(out)
}

/// No-embeddings fallback: scans every chunk's resolved text for
/// case-insensitive token matches (from `extract_search_tokens`), ranked by
/// a weighted occurrence sum. When no tokens are extracted, falls back to
/// the whole query as a single needle (backward-compatible for one-word
/// queries). Scores are not comparable to the semantic tier's cosine
/// similarity.
fn lexical_matches(
    chunk_index: &ChunkIndex,
    scope: &ToolScope,
    query: &str,
    top_k: usize,
) -> Vec<ToolMatch> {
    let tokens = extract_search_tokens(query);
    let needles: Vec<(String, f32)> = if tokens.is_empty() {
        let whole = query.trim().to_lowercase();
        if whole.is_empty() {
            return Vec::new();
        }
        vec![(whole, 1.0)]
    } else {
        tokens
            .iter()
            .map(|t| (t.to_lowercase(), lexical_token_weight(t)))
            .collect()
    };

    let mut scored: Vec<(f32, ChunkMetadata, String)> = Vec::new();
    for metadata in chunk_index.all() {
        if !scope.allows_search_result(&metadata.file_id) {
            continue;
        }
        let Ok(text) = resolve_text(&scope.repo_root, &metadata) else {
            continue;
        };
        let lower = text.to_lowercase();
        let mut score = 0.0_f32;
        for (needle, weight) in &needles {
            let count = lower.matches(needle.as_str()).count() as f32;
            score += count * weight;
        }
        if score > 0.0 {
            scored.push((score, metadata, text));
        }
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(top_k);

    scored
        .into_iter()
        .filter_map(|(score, metadata, text)| {
            let access_path = to_access_relative(scope, &metadata.file_id.0)?;
            Some(ToolMatch {
                path: access_path,
                snippet: truncate_snippet(&text),
                score,
                start_byte: metadata.start_byte,
                end_byte: metadata.end_byte,
                qualified_name: metadata.qualified_name,
                source: MatchSource::Lexical,
            })
        })
        .collect()
}

/// Score for an exact symbol-name hit.
const SYMBOL_NAME_SCORE: f32 = 1.0;
/// Score for a stem/fuzzy symbol-name hit (e.g. notifications ⊂ NotificationService).
const SYMBOL_STEM_SCORE: f32 = 0.95;
/// Score for a path-segment match (slightly below stem).
const SYMBOL_PATH_SCORE: f32 = 0.9;

/// Cheapest tier: exact (case-insensitive) symbol-name matches for each
/// extracted token, stem/fuzzy symbol matches, plus path-segment matches
/// against indexed file paths. Always tried first, regardless of whether
/// the embedding index is ready.
fn symbol_matches(
    repo_index: &RepositoryIndex,
    scope: &ToolScope,
    query: &str,
    top_k: usize,
) -> Vec<ToolMatch> {
    let mut tokens = extract_search_tokens(query);
    // Backward compat: a single exact name with no separators still works
    // even when extract yields nothing unusual (e.g. "UserService" is
    // PascalCase and is extracted; "userservice" all-lowercase plain ≥ 3
    // is also extracted). If the whole trimmed query is a single ASCII
    // word not already listed, include it so `find_symbol` still sees it.
    let trimmed = query.trim();
    if !trimmed.is_empty()
        && trimmed.chars().all(|c| c.is_ascii_alphanumeric())
        && !tokens.iter().any(|t| t.eq_ignore_ascii_case(trimmed))
    {
        tokens.push(trimmed.to_string());
    }
    if tokens.is_empty() {
        return Vec::new();
    }

    // Dedupe key: (access_path, start_byte). Exact (1.0) > stem (0.95) >
    // path (0.9) for the same range/file.
    let mut best: std::collections::HashMap<(String, u32), ToolMatch> =
        std::collections::HashMap::new();

    let insert_candidate =
        |best: &mut std::collections::HashMap<(String, u32), ToolMatch>, candidate: ToolMatch| {
            let key = (candidate.path.clone(), candidate.start_byte);
            best.entry(key)
                .and_modify(|existing| {
                    if candidate.score > existing.score {
                        *existing = candidate.clone();
                    }
                })
                .or_insert(candidate);
        };

    // Exact name lookups (fast path via index).
    for token in &tokens {
        for (file_id, symbol) in repo_index.find_symbol(token) {
            if !scope.allows_search_result(&file_id) {
                continue;
            }
            let Some(access_path) = to_access_relative(scope, &file_id.0) else {
                continue;
            };
            let all_symbols = match repo_index.get(&file_id) {
                Some(f) => f.symbols,
                None => continue,
            };
            let qualified_name =
                qualified_name_for(&symbol, &all_symbols).or_else(|| Some(symbol.name.clone()));
            let snippet =
                read_symbol_snippet(&scope.repo_root, &file_id, &symbol).unwrap_or_default();
            insert_candidate(
                &mut best,
                ToolMatch {
                    path: access_path,
                    snippet,
                    score: SYMBOL_NAME_SCORE,
                    start_byte: symbol.start_byte,
                    end_byte: symbol.end_byte,
                    qualified_name,
                    source: MatchSource::Symbol,
                },
            );
        }
    }

    // Stem/fuzzy: walk indexed symbols once per token that wasn't an exact
    // hit for every possible name (notifications → CollectNotificationService).
    for token in &tokens {
        for (file_id, indexed) in repo_index.all_files() {
            if !scope.allows_search_result(&file_id) {
                continue;
            }
            let Some(access_path) = to_access_relative(scope, &file_id.0) else {
                continue;
            };
            for symbol in &indexed.symbols {
                match symbol_name_matches_token(&symbol.name, token) {
                    MatchTightness::None | MatchTightness::Exact => continue,
                    MatchTightness::Stem => {}
                }
                let qualified_name = qualified_name_for(symbol, &indexed.symbols)
                    .or_else(|| Some(symbol.name.clone()));
                let snippet =
                    read_symbol_snippet(&scope.repo_root, &file_id, symbol).unwrap_or_default();
                insert_candidate(
                    &mut best,
                    ToolMatch {
                        path: access_path.clone(),
                        snippet,
                        score: SYMBOL_STEM_SCORE,
                        start_byte: symbol.start_byte,
                        end_byte: symbol.end_byte,
                        qualified_name,
                        source: MatchSource::Symbol,
                    },
                );
            }
        }
    }

    // Path-segment matches — only for files not already covered by a
    // symbol hit at any byte range.
    for token in &tokens {
        for (file_id, _indexed) in repo_index.all_files() {
            if !scope.allows_search_result(&file_id) {
                continue;
            }
            if !path_segment_matches(&file_id.0, token) {
                continue;
            }
            let Some(access_path) = to_access_relative(scope, &file_id.0) else {
                continue;
            };
            if best.keys().any(|(p, _)| p == &access_path) {
                continue;
            }
            let snippet =
                read_file_first_line_snippet(&scope.repo_root, &file_id).unwrap_or_default();
            insert_candidate(
                &mut best,
                ToolMatch {
                    path: access_path,
                    snippet,
                    score: SYMBOL_PATH_SCORE,
                    start_byte: 0,
                    end_byte: 0,
                    qualified_name: None,
                    source: MatchSource::Symbol,
                },
            );
        }
    }

    let mut out: Vec<ToolMatch> = best.into_values().collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    out.truncate(top_k);
    out
}

/// Best-effort first line of a file for path-segment symbol hits.
fn read_file_first_line_snippet(scope_root: &Path, file_id: &FileId) -> Option<String> {
    let path = scope_root.join(&file_id.0);
    let content = fs::read_to_string(&path).ok()?;
    let line = content.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return None;
    }
    Some(truncate_snippet(line))
}

/// Best-effort slice of `[symbol.start_byte..symbol.end_byte)` off whatever
/// is on disk right now. Unlike `chunk_text::resolve_text`, there's no
/// per-symbol content hash to check staleness against (`Symbol` carries no
/// hash, only `IndexedFile.metadata.hash` does, at the whole-file level) —
/// this simply returns `None` (dropping the snippet, not the whole match)
/// if the byte range is no longer valid for the file's current content.
fn read_symbol_snippet(scope_root: &Path, file_id: &FileId, symbol: &Symbol) -> Option<String> {
    let path = scope_root.join(&file_id.0);
    let content = fs::read_to_string(&path).ok()?;
    let start = symbol.start_byte as usize;
    let end = symbol.end_byte as usize;
    if end > content.len()
        || start > end
        || !content.is_char_boundary(start)
        || !content.is_char_boundary(end)
    {
        return None;
    }
    Some(truncate_snippet(&content[start..end]))
}

fn truncate_snippet(text: &str) -> String {
    if text.len() <= SNIPPET_MAX_CHARS {
        return text.to_string();
    }
    let mut end = SNIPPET_MAX_CHARS;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Builds a `repo_root/docs/...` + `repo_root/src/...` fixture and
    /// returns `(repo_root, docs_root)`, both canonicalized. This file has
    /// far more parallel fixture-based tests than a nanosecond timestamp
    /// alone reliably disambiguates on a coarser system clock — the counter
    /// guarantees uniqueness within the process regardless of clock
    /// resolution.
    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture_repo() -> (PathBuf, PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let repo = std::env::temp_dir().join(format!("alfa-atlas-ai-tools-{nanos}-{n}"));
        let docs = repo.join("docs");
        let src = repo.join("src");
        fs::create_dir_all(&docs).unwrap();
        fs::create_dir_all(&src).unwrap();
        fs::write(docs.join("intro.adoc"), "= Intro\n").unwrap();
        fs::write(docs.join("script.py"), "print('unsupported ext')\n").unwrap();
        fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();

        let repo = repo.canonicalize().unwrap();
        let docs = docs.canonicalize().unwrap();
        (repo, docs)
    }

    // --- `related_files` ---

    #[test]
    fn related_files_combines_java_imports_and_workspace_includes() {
        let (repo, docs) = fixture_repo();

        // JSON `$ref` side: `current.json` -> `related.json`.
        fs::write(docs.join("current.json"), r#"{"$ref": "./related.json"}"#).unwrap();
        fs::write(docs.join("related.json"), "{}").unwrap();

        let workspace_index =
            Arc::new(WorkspaceIndex::new(crate::infra::parsers::registry::ParserRegistry::new()));
        workspace_index.build(repo.clone()).unwrap();

        // Java side: `Current.java` imports `com.example.Related` —
        // `java_dependencies` matches on the literal on-disk path, so the
        // package directory layout must actually match `com/example/`.
        let pkg = repo.join("src/com/example");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("Current.java"), "import com.example.Related;\nclass Current {}\n").unwrap();
        fs::write(pkg.join("Related.java"), "package com.example;\nclass Related {}\n").unwrap();
        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();

        let deps = EmbeddingDeps {
            workspace_index,
            repo_index: Arc::new(repo_index),
            ..EmbeddingDeps::empty()
        };

        let json_related = related_files(&deps, &FileId("docs/current.json".to_string()));
        assert!(json_related.contains(&FileId("docs/related.json".to_string())));

        let java_related = related_files(&deps, &FileId("src/com/example/Current.java".to_string()));
        assert!(java_related.contains(&FileId("src/com/example/Related.java".to_string())));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn related_files_is_empty_for_an_unknown_file() {
        let deps = EmbeddingDeps::empty();
        assert!(related_files(&deps, &FileId("nowhere.json".to_string())).is_empty());
    }

    // --- `apply_related_boost` ---

    fn sample_match(path: &str, score: f32) -> ToolMatch {
        ToolMatch {
            path: path.to_string(),
            snippet: String::new(),
            score,
            start_byte: 0,
            end_byte: 0,
            qualified_name: None,
            source: MatchSource::Lexical,
        }
    }

    #[test]
    fn apply_related_boost_reorders_a_related_match_above_a_stronger_unrelated_one() {
        let matches = vec![sample_match("unrelated.json", 6.0), sample_match("related.json", 5.0)];
        let related: HashSet<FileId> = [FileId("related.json".to_string())].into_iter().collect();

        // `5.0 * RELATED_FILE_BOOST` (`1.25`) = `6.25`, just enough to edge
        // out the unboosted `6.0`.
        let boosted = apply_related_boost(matches, &related, 2);

        assert_eq!(boosted[0].path, "related.json");
        assert_eq!(boosted[1].path, "unrelated.json");
    }

    #[test]
    fn apply_related_boost_is_a_no_op_with_no_related_files() {
        let matches = vec![sample_match("a.json", 6.0), sample_match("b.json", 5.0)];

        let unboosted = apply_related_boost(matches, &HashSet::new(), 2);

        assert_eq!(unboosted[0].path, "a.json");
        assert_eq!(unboosted[0].score, 6.0);
        assert_eq!(unboosted[1].path, "b.json");
        assert_eq!(unboosted[1].score, 5.0);
    }

    #[test]
    fn apply_related_boost_truncates_to_budget_after_resorting() {
        let matches = vec![sample_match("unrelated.json", 6.0), sample_match("related.json", 5.0)];
        let related: HashSet<FileId> = [FileId("related.json".to_string())].into_iter().collect();

        let boosted = apply_related_boost(matches, &related, 1);

        assert_eq!(boosted.len(), 1);
        assert_eq!(boosted[0].path, "related.json");
    }

    #[test]
    fn render_file_tree_nests_by_path_and_sorts_children() {
        let entries = vec![
            ToolFileEntry { path: "build.gradle".to_string(), is_dir: false },
            ToolFileEntry { path: "src/main/java/com/example/Application.java".to_string(), is_dir: false },
            ToolFileEntry { path: "src/main/java/com/example/UserService.java".to_string(), is_dir: false },
            ToolFileEntry { path: "src/main/resources/application.yml".to_string(), is_dir: false },
            ToolFileEntry { path: "src/test/java/com/example/UserServiceTest.java".to_string(), is_dir: false },
        ];

        let tree = render_file_tree(&entries);

        assert_eq!(
            tree,
            "./\n\
             ├── build.gradle\n\
             └── src/\n    \
             ├── main/\n    │   \
             ├── java/\n    │   │   \
             └── com/\n    │   │       \
             └── example/\n    │   │           \
             ├── Application.java\n    │   │           \
             └── UserService.java\n    │   \
             └── resources/\n    │       \
             └── application.yml\n    \
             └── test/\n        \
             └── java/\n            \
             └── com/\n                \
             └── example/\n                    \
             └── UserServiceTest.java\n"
        );
    }

    #[test]
    fn render_file_tree_marks_explicit_empty_directory() {
        let entries = vec![ToolFileEntry { path: "empty".to_string(), is_dir: true }];
        assert_eq!(render_file_tree(&entries), "./\n└── empty/\n");
    }

    /// Calls `execute_tool` for `ReadFile` and unwraps the expected
    /// `ToolResult::File` shape, so tests read like the plain `read_file`
    /// calls they replaced while still exercising the real public entry
    /// point (allowlist check included).
    fn read(scope: &ToolScope, path: &str) -> Result<String, ToolError> {
        match execute_tool(
            scope,
            ToolCall::ReadFile(ReadFileArgs {
                path: path.to_string(),
                start_line: None,
                end_line: None,
            }),
            &EmbeddingDeps::empty(),
            &[],
        )? {
            ToolResult::File { content, .. } => Ok(content),
            other => panic!("expected ToolResult::File, got {other:?}"),
        }
    }

    /// Like `read`, but returns the full `ToolResult` so range/total-line
    /// metadata is inspectable, and takes an explicit line range.
    fn read_range(
        scope: &ToolScope,
        path: &str,
        start_line: Option<u32>,
        end_line: Option<u32>,
    ) -> Result<ToolResult, ToolError> {
        execute_tool(
            scope,
            ToolCall::ReadFile(ReadFileArgs {
                path: path.to_string(),
                start_line,
                end_line,
            }),
            &EmbeddingDeps::empty(),
            &[],
        )
    }

    fn list(scope: &ToolScope, path: Option<&str>) -> Result<Vec<ToolFileEntry>, ToolError> {
        list_scoped(scope, path, None, None)
    }

    fn list_scoped(
        scope: &ToolScope,
        path: Option<&str>,
        depth: Option<u32>,
        pattern: Option<&str>,
    ) -> Result<Vec<ToolFileEntry>, ToolError> {
        match execute_tool(
            scope,
            ToolCall::ListFiles(ListFilesArgs {
                path: path.map(str::to_string),
                depth,
                pattern: pattern.map(str::to_string),
            }),
            &EmbeddingDeps::empty(),
            &[],
        )? {
            ToolResult::FileList(entries) => Ok(entries),
            other => panic!("expected ToolResult::FileList, got {other:?}"),
        }
    }

    fn write(scope: &ToolScope, path: &str, content: &str) -> Result<String, ToolError> {
        match execute_tool(
            scope,
            ToolCall::WriteFile(WriteFileArgs {
                path: path.to_string(),
                content: content.to_string(),
            }),
            &EmbeddingDeps::empty(),
            &[],
        )? {
            ToolResult::FileWritten { path, .. } => Ok(path),
            other => panic!("expected ToolResult::FileWritten, got {other:?}"),
        }
    }

    fn edit(scope: &ToolScope, path: &str, edits: Vec<(&str, &str)>) -> Result<String, ToolError> {
        match execute_tool(
            scope,
            ToolCall::EditFile(EditFileArgs {
                path: path.to_string(),
                edits: edits
                    .into_iter()
                    .map(|(old, new)| FileEdit { old: old.to_string(), new: new.to_string() })
                    .collect(),
            }),
            &EmbeddingDeps::empty(),
            &[],
        )? {
            ToolResult::FileEdited { path, .. } => Ok(path),
            other => panic!("expected ToolResult::FileEdited, got {other:?}"),
        }
    }

    fn create_dir(scope: &ToolScope, path: &str) -> Result<String, ToolError> {
        create_dir_with_template(scope, path, None).map(|(path, _, _)| path)
    }

    fn create_dir_with_template(
        scope: &ToolScope,
        path: &str,
        template: Option<&str>,
    ) -> Result<(String, Option<String>, Vec<String>), ToolError> {
        match execute_tool(
            scope,
            ToolCall::CreateDirectory(CreateDirectoryArgs {
                path: path.to_string(),
                template: template.map(str::to_string),
            }),
            &EmbeddingDeps::empty(),
            &[],
        )? {
            ToolResult::DirectoryCreated {
                path,
                template,
                created_files,
            } => Ok((path, template, created_files)),
            other => panic!("expected ToolResult::DirectoryCreated, got {other:?}"),
        }
    }

    fn delete(scope: &ToolScope, path: &str) -> Result<String, ToolError> {
        match execute_tool(
            scope,
            ToolCall::DeleteFile(DeleteFileArgs { path: path.to_string() }),
            &EmbeddingDeps::empty(),
            &[],
        )? {
            ToolResult::FileDeleted { path, .. } => Ok(path),
            other => panic!("expected ToolResult::FileDeleted, got {other:?}"),
        }
    }

    fn delete_dir(scope: &ToolScope, path: &str, recursive: Option<bool>) -> Result<String, ToolError> {
        match execute_tool(
            scope,
            ToolCall::DeleteDirectory(DeleteDirectoryArgs { path: path.to_string(), recursive }),
            &EmbeddingDeps::empty(),
            &[],
        )? {
            ToolResult::DirectoryDeleted { path } => Ok(path),
            other => panic!("expected ToolResult::DirectoryDeleted, got {other:?}"),
        }
    }

    fn move_it(
        scope: &ToolScope,
        path: &str,
        new_path: &str,
    ) -> Result<(String, String, Vec<UpdatedReference>), ToolError> {
        move_it_with_deps(scope, path, new_path, &EmbeddingDeps::empty())
    }

    fn move_it_with_deps(
        scope: &ToolScope,
        path: &str,
        new_path: &str,
        deps: &EmbeddingDeps,
    ) -> Result<(String, String, Vec<UpdatedReference>), ToolError> {
        match execute_tool(
            scope,
            ToolCall::Move(MoveArgs { path: path.to_string(), new_path: new_path.to_string() }),
            deps,
            &[],
        )? {
            ToolResult::Moved { from, to, updated_files } => Ok((from, to, updated_files)),
            other => panic!("expected ToolResult::Moved, got {other:?}"),
        }
    }

    /// A `WorkspaceIndex` built from a real walk of `repo_root` — for the
    /// one `move` test that needs `deps.workspace_index` to actually know
    /// about the fixture's documents (everything else uses
    /// `EmbeddingDeps::empty()`'s blank one, since `move`'s reference
    /// rewrite is a no-op — empty `updated_files` — against a blank
    /// index, exercised by the other `move_*` tests below).
    fn build_test_workspace_index(repo_root: &Path) -> Arc<WorkspaceIndex> {
        let idx = Arc::new(WorkspaceIndex::new(
            crate::infra::parsers::registry::ParserRegistry::new(),
        ));
        idx.build(repo_root.to_path_buf()).unwrap();
        idx
    }

    #[test]
    fn read_file_inside_docs_root_succeeds_in_docs_only() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let content = read(&scope, "intro.adoc").unwrap();
        assert_eq!(content, "= Intro\n");
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_on_a_directory_returns_not_a_file() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = read(&scope, ".").unwrap_err();
        assert!(matches!(err, ToolError::NotAFile(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_rejects_parent_escape_in_both_modes() {
        let (repo, docs) = fixture_repo();

        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let err = read(&docs_only, "../src/main.rs").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let err = read(&full_repo, "../outside.txt").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    /// Regression test: a model guessing at a plausible-but-wrong nested
    /// path (e.g. `components/schemas/all.yaml` in a repo whose schemas
    /// actually live directly at `schemas/all.yaml`) must see a clean
    /// `NotFound` it can react to — not `ToolError::Io` wrapping a raw
    /// `"No such file or directory (os error 2)"` with no path in it, which
    /// is what `paths::ensure_under` used to surface whenever *more* than
    /// the immediate parent directory of a `readFile`/`listFiles` path was
    /// missing.
    #[test]
    fn read_file_missing_several_directories_deep_returns_clean_not_found() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = read(&scope, "components/schemas/all.yaml").unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)), "expected NotFound, got {err:?}");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_missing_several_directories_deep_returns_clean_not_found() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = list(&scope, Some("components/schemas")).unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)), "expected NotFound, got {err:?}");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_same_relative_path_resolves_against_different_roots_by_mode() {
        let (repo, docs) = fixture_repo();

        // "src/main.rs" only exists under `repo`, not under `docs` — so the
        // same relative path is simply absent from the docs-only root
        // (there is no `docs/src/main.rs`), while it resolves fine once the
        // scope root widens to the whole repo. This is `ToolScope`'s mode
        // switch doing its job, not a `..`-escape (that's covered above).
        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        assert!(read(&docs_only, "src/main.rs").is_err());

        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let content = read(&full_repo, "src/main.rs").unwrap();
        assert_eq!(content, "fn main() {}\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn write_file_creates_and_overwrites_under_docs_root() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        write(&scope, "new.adoc", "= New\n").unwrap();
        assert_eq!(fs::read_to_string(docs.join("new.adoc")).unwrap(), "= New\n");

        write(&scope, "new.adoc", "= Replaced\n").unwrap();
        assert_eq!(fs::read_to_string(docs.join("new.adoc")).unwrap(), "= Replaced\n");

        fs::remove_dir_all(&repo).ok();
    }

    /// The load-bearing containment guarantee for `WriteFile`: even in
    /// `FullRepo` mode (where `ReadFile`/`ListFiles` resolve against
    /// `repo_root`), a write must land under `docs_root` — `FullRepo` widens
    /// read context, not write license.
    #[test]
    fn write_file_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        // Full-repo paths are repo-relative (same as readFile).
        let written = write(&full_repo, "docs/guide.adoc", "= Guide\n").unwrap();
        assert_eq!(written, "docs/guide.adoc");

        assert_eq!(fs::read_to_string(docs.join("guide.adoc")).unwrap(), "= Guide\n");
        assert!(!repo.join("guide.adoc").exists());

        // Bare docs-relative path resolves under the repo root → outside docs.
        let err = write(&full_repo, "guide.adoc", "= Nope\n").unwrap_err();
        assert!(matches!(err, ToolError::OutsideDocumentation(_)), "got {err:?}");
        let err = write(&full_repo, "src/main.rs", "fn main() {}\n").unwrap_err();
        assert!(matches!(err, ToolError::OutsideDocumentation(_)), "got {err:?}");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn write_file_rejects_unsupported_extension() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = write(&scope, "notes.py", "print('nope')\n").unwrap_err();
        assert!(matches!(err, ToolError::Io(_)));
        assert!(!docs.join("notes.py").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn write_file_rejects_atlas_memory_store() {
        let (repo, _docs) = fixture_repo();
        // When docs root is the repo itself, `.txt` under `.atlas/memory`
        // would otherwise be a writable supported path.
        let scope = ToolScope::for_project(&repo, &repo, AiAccessMode::DocsOnly);
        let err = write(&scope, ".atlas/memory/LOG.txt", "hijack\n").unwrap_err();
        assert!(
            matches!(err, ToolError::PathEscape(ref p) if p.contains(".atlas/memory")),
            "got {err:?}"
        );
        assert!(!repo.join(".atlas/memory/LOG.txt").exists());
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn write_file_rejects_path_escape() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = write(&scope, "../outside.adoc", "= Leak\n").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));
        assert!(!repo.join("outside.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_applies_a_single_replacement() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "private String name;\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        edit(
            &scope,
            "class.adoc",
            vec![("private String name;", "private String name;\nprivate int age;")],
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(docs.join("class.adoc")).unwrap(),
            "private String name;\nprivate int age;\n"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_applies_multiple_independent_edits_in_one_call() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\ngamma\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        edit(&scope, "class.adoc", vec![("alpha", "ALPHA"), ("gamma", "GAMMA")]).unwrap();
        assert_eq!(
            fs::read_to_string(docs.join("class.adoc")).unwrap(),
            "ALPHA\nbeta\nGAMMA\n"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_missing_file() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = edit(&scope, "does-not-exist.adoc", vec![("a", "b")]).unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_old_text_not_found_and_writes_nothing() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = edit(&scope, "class.adoc", vec![("nope", "NOPE")]).unwrap_err();
        assert!(matches!(err, ToolError::EditTextNotFound(_)));
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "alpha\nbeta\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_ambiguous_old_text_and_writes_nothing() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "dup\ndup\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = edit(&scope, "class.adoc", vec![("dup", "DUP")]).unwrap_err();
        assert!(matches!(err, ToolError::EditTextAmbiguous(_, 2)));
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "dup\ndup\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_overlapping_edits_and_writes_nothing() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "abcdef\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // "abcd" (0..4) and "cdef" (2..6) overlap on "cd".
        let err = edit(&scope, "class.adoc", vec![("abcd", "X"), ("cdef", "Y")]).unwrap_err();
        assert!(matches!(err, ToolError::EditsOverlap));
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "abcdef\n");

        fs::remove_dir_all(&repo).ok();
    }

    /// Edits are validated against the file's *original* content, not
    /// sequentially against each other's output — an edit whose `new` text
    /// happens to equal another edit's `old` text must not make that other
    /// edit's match count look any different.
    #[test]
    fn edit_file_validates_edits_against_original_content_not_sequentially() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // If edits were applied sequentially, "alpha" -> "beta" would make
        // "beta" ambiguous (two occurrences) by the time the second edit's
        // match is checked. Validated against the original, "beta" still
        // matches exactly once.
        edit(&scope, "class.adoc", vec![("alpha", "beta"), ("beta", "BETA")]).unwrap();
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "beta\nBETA\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("guide.adoc"), "old text\n").unwrap();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        edit(&full_repo, "docs/guide.adoc", vec![("old text", "new text")]).unwrap();
        assert_eq!(fs::read_to_string(docs.join("guide.adoc")).unwrap(), "new text\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_path_escape() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = edit(&scope, "../outside.adoc", vec![("a", "b")]).unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    /// A canned `LlmProvider` for `EditFile`'s fast-apply fallback tests —
    /// `chat` returns whatever `response` says regardless of the request,
    /// and counts how many times it was called so a test can assert the
    /// fallback was (or, more often, was *not*) actually reached.
    /// `chat_stream`/`list_models` are never used by fast-apply, so they
    /// panic if a bug ever routes a call through them instead.
    struct MockFastApplyProvider {
        response: Result<String, String>,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl MockFastApplyProvider {
        fn returning(content: &str) -> Self {
            Self { response: Ok(content.to_string()), calls: std::sync::atomic::AtomicUsize::new(0) }
        }

        fn failing(message: &str) -> Self {
            Self { response: Err(message.to_string()), calls: std::sync::atomic::AtomicUsize::new(0) }
        }
    }

    impl crate::domain::llm::LlmProvider for MockFastApplyProvider {
        fn chat(
            &self,
            _request: crate::domain::llm::ChatRequest,
        ) -> Result<crate::domain::llm::ChatResponse, crate::domain::llm::LlmError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match &self.response {
                Ok(content) => {
                    Ok(crate::domain::llm::ChatResponse {
                        content: Some(content.clone()),
                        tool_calls: vec![],
                        usage: None,
                    })
                }
                Err(message) => Err(crate::domain::llm::LlmError::Provider(message.clone())),
            }
        }

        fn chat_stream(
            &self,
            _request: crate::domain::llm::ChatRequest,
            _on_delta: &dyn Fn(&str),
            _on_reasoning: &dyn Fn(&str),
            _cancelled: &dyn Fn() -> bool,
        ) -> Result<crate::domain::llm::ChatStreamResult, crate::domain::llm::LlmError> {
            unimplemented!("fast-apply only ever calls chat(), never chat_stream()")
        }

        fn list_models(&self) -> Result<Vec<crate::domain::llm::LlmModelInfo>, crate::domain::llm::LlmError> {
            unimplemented!("fast-apply only ever calls chat(), never list_models()")
        }
    }

    fn edit_with_fast_apply(
        scope: &ToolScope,
        path: &str,
        edits: Vec<(&str, &str)>,
        provider: Arc<dyn LlmProvider>,
    ) -> Result<String, ToolError> {
        let deps = EmbeddingDeps {
            fast_apply: Some((provider, "test-model".to_string())),
            ..EmbeddingDeps::empty()
        };
        match execute_tool(
            scope,
            ToolCall::EditFile(EditFileArgs {
                path: path.to_string(),
                edits: edits
                    .into_iter()
                    .map(|(old, new)| FileEdit { old: old.to_string(), new: new.to_string() })
                    .collect(),
            }),
            &deps,
            &[],
        )? {
            ToolResult::FileEdited { path, .. } => Ok(path),
            other => panic!("expected ToolResult::FileEdited, got {other:?}"),
        }
    }

    #[test]
    fn edit_file_fast_apply_reconciles_a_non_exact_old_text() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\ngamma\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // "Beta" (wrong case) never matches "beta" exactly — deterministic
        // matching alone would reject this with `EditTextNotFound`.
        let mock = Arc::new(MockFastApplyProvider::returning("alpha\nBETA\ngamma\n"));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        edit_with_fast_apply(&scope, "class.adoc", vec![("Beta", "BETA")], provider).unwrap();

        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "alpha\nBETA\ngamma\n");
        assert_eq!(mock.calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_fast_apply_is_not_used_when_exact_match_succeeds() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let mock = Arc::new(MockFastApplyProvider::returning("should never be read"));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        edit_with_fast_apply(&scope, "class.adoc", vec![("alpha", "ALPHA")], provider).unwrap();

        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "ALPHA\nbeta\n");
        assert_eq!(mock.calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_fast_apply_strips_a_wrapping_code_fence_from_model_output() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let mock = Arc::new(MockFastApplyProvider::returning("```\nalpha\nBETA\n```"));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        edit_with_fast_apply(&scope, "class.adoc", vec![("Beta", "BETA")], provider).unwrap();

        // No trailing newline: `strip_code_fence` splits on `.lines()`,
        // which discards line-terminator information, so a newline
        // immediately before the closing fence isn't recoverable.
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "alpha\nBETA");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_fast_apply_rejects_output_with_no_change_and_writes_nothing() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let mock = Arc::new(MockFastApplyProvider::returning("alpha\nbeta\n"));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        let err =
            edit_with_fast_apply(&scope, "class.adoc", vec![("Beta", "BETA")], provider).unwrap_err();
        match err {
            ToolError::EditApplyFailed(_, reason) => assert!(reason.contains("no change")),
            other => panic!("expected EditApplyFailed, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "alpha\nbeta\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_fast_apply_rejects_output_that_changes_unrelated_content_and_writes_nothing() {
        let (repo, docs) = fixture_repo();
        let filler = "Unrelated filler sentence stays put. ".repeat(20);
        // Lowercase "beta" so the edit's ("Beta") `old` text does *not*
        // already match exactly — otherwise the deterministic fast path
        // would apply it directly and the mock would never be consulted.
        let original = format!("Intro.\n\nbeta needs fixing.\n\n{filler}\n");
        fs::write(docs.join("class.adoc"), &original).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // The model correctly fixes "Beta" but also rewrites the large
        // unrelated paragraph — far more of the file changed than this
        // edit accounts for, so the whole call must be rejected rather
        // than silently accepting a corrupted rewrite.
        let rewritten_filler = "Something completely different now. ".repeat(20);
        let candidate = format!("Intro.\n\nBETA needs fixing.\n\n{rewritten_filler}\n");
        let mock = Arc::new(MockFastApplyProvider::returning(&candidate));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        let err =
            edit_with_fast_apply(&scope, "class.adoc", vec![("Beta", "BETA")], provider).unwrap_err();
        match err {
            ToolError::EditApplyFailed(_, reason) => assert!(reason.contains("changed more")),
            other => panic!("expected EditApplyFailed, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), original);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_fast_apply_surfaces_a_provider_error() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let mock = Arc::new(MockFastApplyProvider::failing("network unreachable"));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        let err =
            edit_with_fast_apply(&scope, "class.adoc", vec![("Beta", "BETA")], provider).unwrap_err();
        match err {
            ToolError::EditApplyFailed(_, reason) => assert!(reason.contains("provider error")),
            other => panic!("expected EditApplyFailed, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "alpha\nbeta\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_fast_apply_resolves_an_ambiguous_old_text() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "dup\ndup\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // "dup" matches twice — deterministic matching alone would reject
        // this with `EditTextAmbiguous`. The model is trusted to pick the
        // right one from context; the safety check only bounds *how much*
        // changed, not *which* occurrence.
        let mock = Arc::new(MockFastApplyProvider::returning("DUP\ndup\n"));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        edit_with_fast_apply(&scope, "class.adoc", vec![("dup", "DUP")], provider).unwrap();

        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "DUP\ndup\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn create_directory_creates_missing_parents_under_docs_root() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        create_dir(&scope, "guides/nested").unwrap();

        assert!(docs.join("guides/nested").is_dir());

        fs::remove_dir_all(&repo).ok();
    }

    /// Same containment guarantee as `write_file_full_repo_mode_still_targets_docs_root_not_repo_root`:
    /// even in `FullRepo` mode a new directory must land under `docs_root`.
    #[test]
    fn create_directory_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        create_dir(&full_repo, "docs/endpoints").unwrap();

        assert!(docs.join("endpoints").is_dir());
        assert!(!repo.join("endpoints").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn create_directory_rejects_an_already_existing_path() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        create_dir(&scope, "guides").unwrap();
        let err = create_dir(&scope, "guides").unwrap_err();
        // `ProjectError::AlreadyExists` gained an explicit `ToolError`
        // mapping once `Move` needed to distinguish it from a generic IO
        // failure — `CreateDirectory` picks up the same precision as a
        // side effect, no longer the generic `Io` catch-all.
        assert!(matches!(err, ToolError::AlreadyExists(_)));

        // Also rejected when the path is already occupied by a file.
        let err = create_dir(&scope, "intro.adoc").unwrap_err();
        assert!(matches!(err, ToolError::AlreadyExists(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn create_directory_rejects_path_escape() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // `reject_atlas_memory_path` runs before `create_project_dir` and
        // itself calls `paths::join_relative` on the raw arg, which rejects
        // a `..` component with `ProjectError::PathEscape` — mapped
        // directly to `ToolError::PathEscape` by `From<ProjectError> for
        // ToolError`, so `create_project_dir`'s own
        // `validate_relative_name`/`InvalidName` path is never reached.
        let err = create_dir(&scope, "../outside-dir").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));
        assert!(!repo.join("outside-dir").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn create_directory_rest_endpoint_template_creates_scaffold_files() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let (path, template, created_files) =
            create_dir_with_template(&scope, "api/getUser", Some("restEndpoint")).unwrap();

        assert_eq!(path, "api/getUser");
        assert_eq!(template.as_deref(), Some("restEndpoint"));
        assert_eq!(
            created_files,
            vec![
                "api/getUser/getUser.adoc",
                "api/getUser/request.adoc",
                "api/getUser/response.adoc",
                "api/getUser/getUser.puml",
            ]
        );
        for rel in &created_files {
            assert!(docs.join(rel).is_file(), "missing scaffold file {rel}");
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn create_directory_rejects_unknown_template() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = create_dir_with_template(&scope, "api/foo", Some("openapi")).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { .. }));
        assert!(!docs.join("api/foo").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_file_removes_the_file() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        delete(&scope, "intro.adoc").unwrap();
        assert!(!docs.join("intro.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_file_rejects_missing_file() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = delete(&scope, "does-not-exist.adoc").unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_file_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        delete(&full_repo, "docs/intro.adoc").unwrap();
        assert!(!docs.join("intro.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_file_rejects_path_escape() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // `delete_file` now reads the file first (to diff against on
        // deletion — see `text_diff::diff_stats`), and `read_project_file`
        // validates via `join_relative` rather than `validate_relative_name`
        // — so `..` surfaces as `PathEscape` here rather than the
        // `InvalidName`-via-`Io` catch-all `delete_project_file` alone would
        // have produced. Equally rejected either way; this is just the more
        // specific variant.
        let err = delete(&scope, "../outside.adoc").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_directory_removes_an_empty_directory_by_default() {
        let (repo, docs) = fixture_repo();
        create_dir(&ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly), "empty").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        delete_dir(&scope, "empty", None).unwrap();
        assert!(!docs.join("empty").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_directory_refuses_a_non_empty_directory_by_default() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("folder")).unwrap();
        fs::write(docs.join("folder/note.adoc"), "= Note\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = delete_dir(&scope, "folder", None).unwrap_err();
        assert!(matches!(err, ToolError::DirectoryNotEmpty(_)));
        assert!(docs.join("folder/note.adoc").exists());

        // Explicit `recursive: false` behaves the same as omitting it.
        let err = delete_dir(&scope, "folder", Some(false)).unwrap_err();
        assert!(matches!(err, ToolError::DirectoryNotEmpty(_)));
        assert!(docs.join("folder/note.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_directory_recursive_true_deletes_a_non_empty_directory() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("folder")).unwrap();
        fs::write(docs.join("folder/note.adoc"), "= Note\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        delete_dir(&scope, "folder", Some(true)).unwrap();
        assert!(!docs.join("folder").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_directory_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("empty")).unwrap();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        delete_dir(&full_repo, "docs/empty", None).unwrap();
        assert!(!docs.join("empty").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_directory_rejects_missing_directory() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = delete_dir(&scope, "does-not-exist", None).unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));

        fs::remove_dir_all(&repo).ok();
    }

    /// The other half of the benchmark-surfaced gap: a recursive
    /// `deleteDirectory` used to leave every nested file's row behind in
    /// the index (only the top-level fs subtree was ever touched — nothing
    /// in the AI tool path called `remove_document`), so a `check` right
    /// after still reported diagnostics for files that no longer existed
    /// on disk.
    #[test]
    fn delete_directory_recursive_removes_nested_documents_from_the_index() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("folder/nested")).unwrap();
        fs::write(docs.join("folder/note.adoc"), "= Note\n").unwrap();
        fs::write(docs.join("folder/nested/deep.adoc"), "= Deep\n").unwrap();

        let mut deps = EmbeddingDeps::empty();
        deps.workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        assert!(deps.workspace_index.get_document(&docs.join("folder/nested/deep.adoc")).is_some());

        execute_tool(
            &scope,
            ToolCall::DeleteDirectory(DeleteDirectoryArgs {
                path: "folder".to_string(),
                recursive: Some(true),
            }),
            &deps,
            &[],
        )
        .unwrap();

        assert!(deps.workspace_index.get_document(&docs.join("folder/note.adoc")).is_none());
        assert!(deps.workspace_index.get_document(&docs.join("folder/nested/deep.adoc")).is_none());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_renames_a_file_in_place() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let (from, to, updated_files) = move_it(&scope, "intro.adoc", "introduction.adoc").unwrap();
        assert_eq!(from, "intro.adoc");
        assert_eq!(to, "introduction.adoc");
        assert!(updated_files.is_empty());
        assert!(!docs.join("intro.adoc").exists());
        assert!(docs.join("introduction.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_moves_a_file_to_a_different_directory() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("archive")).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        move_it(&scope, "intro.adoc", "archive/intro.adoc").unwrap();
        assert!(!docs.join("intro.adoc").exists());
        assert!(docs.join("archive/intro.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_moves_and_renames_a_directory_together() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("old/nested")).unwrap();
        fs::write(docs.join("old/nested/page.adoc"), "= Page\n").unwrap();
        // `rename_project_dir`/`fs::rename` need the destination's parent to
        // already exist — unlike `write_project_file`, neither
        // `rename_project_file` nor `rename_project_dir` auto-creates it
        // (the manual drag-and-drop UI never hits this: a drop target is
        // always an existing folder).
        fs::create_dir_all(docs.join("archive")).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        move_it(&scope, "old", "archive/renamed").unwrap();
        assert!(!docs.join("old").exists());
        assert_eq!(
            fs::read_to_string(docs.join("archive/renamed/nested/page.adoc")).unwrap(),
            "= Page\n"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_rejects_when_destination_already_exists() {
        let (repo, docs) = fixture_repo();
        // `script.py` (fixture_repo's other file) is an unsupported
        // extension, which would fail with `UnsupportedFile` before ever
        // reaching the exists check — use another `.adoc` so this test
        // actually exercises `AlreadyExists`.
        fs::write(docs.join("other.adoc"), "= Other\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = move_it(&scope, "intro.adoc", "other.adoc").unwrap_err();
        assert!(matches!(err, ToolError::AlreadyExists(_)));
        // Nothing moved.
        assert!(docs.join("intro.adoc").exists());
        assert_eq!(fs::read_to_string(docs.join("other.adoc")).unwrap(), "= Other\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_rejects_missing_source() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = move_it(&scope, "does-not-exist.adoc", "new-name.adoc").unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        move_it(&full_repo, "docs/intro.adoc", "docs/renamed.adoc").unwrap();
        assert!(docs.join("renamed.adoc").exists());
        assert!(!repo.join("renamed.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_rejects_path_escape() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // `reject_atlas_memory_path` runs before `move_path`'s own path
        // handling and itself calls `paths::join_relative` on the raw arg,
        // which rejects a `..` component with `ProjectError::PathEscape` —
        // mapped directly to `ToolError::PathEscape` (see the matching
        // `create_directory_rejects_path_escape` comment).
        let err = move_it(&scope, "../outside.adoc", "new-name.adoc").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    /// The one test that wires up a real, built `WorkspaceIndex` instead of
    /// `EmbeddingDeps::empty()`'s blank one — proves `move_path` actually
    /// calls through to `reference_rewrite::rewrite_references` and reports
    /// the result, not just performs a bare `fs::rename`.
    #[test]
    fn move_rewrites_references_in_other_files() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("sub")).unwrap();
        fs::write(docs.join("sub/detail.adoc"), "= Detail\n").unwrap();
        fs::write(docs.join("guide.adoc"), "= Guide\n\ninclude::sub/detail.adoc[]\n").unwrap();
        // Same as `move_moves_and_renames_a_directory_together`: the
        // destination's parent must already exist.
        fs::create_dir_all(docs.join("sub2")).unwrap();

        let mut deps = EmbeddingDeps::empty();
        deps.workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let (from, to, updated_files) =
            move_it_with_deps(&scope, "sub/detail.adoc", "sub2/detail.adoc", &deps).unwrap();
        assert_eq!(from, "sub/detail.adoc");
        assert_eq!(to, "sub2/detail.adoc");
        assert_eq!(
            updated_files,
            vec![UpdatedReference { docs_relative_path: "guide.adoc".to_string(), count: 1 }]
        );
        assert!(!docs.join("sub/detail.adoc").exists());
        assert!(docs.join("sub2/detail.adoc").exists());
        assert_eq!(
            fs::read_to_string(docs.join("guide.adoc")).unwrap(),
            "= Guide\n\ninclude::sub2/detail.adoc[]\n"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_of_an_unreferenced_file_reports_zero_updated_references() {
        let (repo, docs) = fixture_repo();
        let mut deps = EmbeddingDeps::empty();
        deps.workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let (_, _, updated_files) =
            move_it_with_deps(&scope, "intro.adoc", "introduction.adoc", &deps).unwrap();
        assert!(updated_files.is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    /// Reproduces the exact gap a benchmark run of the file tools surfaced:
    /// a `writeFile` call creates a file that references another, then a
    /// `move` call in the same turn relocates the referenced file — before
    /// `write_file` synchronously called `update_document`, `move`'s
    /// reference lookup ran against a `WorkspaceIndex` that had never heard
    /// of the just-written file, so it silently reported zero updated
    /// references even though the include was right there on disk.
    #[test]
    fn move_finds_a_reference_written_by_write_file_earlier_in_the_same_turn() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("sub")).unwrap();
        fs::write(docs.join("sub/detail.adoc"), "= Detail\n").unwrap();
        fs::create_dir_all(docs.join("sub2")).unwrap();

        let mut deps = EmbeddingDeps::empty();
        deps.workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // `guide.adoc` doesn't exist yet when the index above is built —
        // it's created through the real tool, same as the AI would.
        execute_tool(
            &scope,
            ToolCall::WriteFile(WriteFileArgs {
                path: "guide.adoc".to_string(),
                content: "= Guide\n\ninclude::sub/detail.adoc[]\n".to_string(),
            }),
            &deps,
            &[],
        )
        .unwrap();

        let (_, _, updated_files) =
            move_it_with_deps(&scope, "sub/detail.adoc", "sub2/detail.adoc", &deps).unwrap();
        assert_eq!(
            updated_files,
            vec![UpdatedReference { docs_relative_path: "guide.adoc".to_string(), count: 1 }]
        );
        assert_eq!(
            fs::read_to_string(docs.join("guide.adoc")).unwrap(),
            "= Guide\n\ninclude::sub2/detail.adoc[]\n"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_updates_the_moved_documents_own_row_in_the_index() {
        let (repo, docs) = fixture_repo();
        let mut deps = EmbeddingDeps::empty();
        deps.workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        move_it_with_deps(&scope, "intro.adoc", "introduction.adoc", &deps).unwrap();

        assert!(deps.workspace_index.get_document(&docs.join("intro.adoc")).is_none());
        assert!(deps.workspace_index.get_document(&docs.join("introduction.adoc")).is_some());

        fs::remove_dir_all(&repo).ok();
    }

    /// `join_relative`'s `..`-rejection only catches lexical traversal; the
    /// real defense against a path that resolves outside the root by other
    /// means (e.g. a symlink) is `ensure_under`'s canonicalize+`starts_with`
    /// check. Exercise that directly so the containment guarantee isn't
    /// only proven for the lexical case.
    #[cfg(unix)]
    #[test]
    fn read_file_rejects_symlink_escaping_docs_root() {
        let (repo, docs) = fixture_repo();
        std::os::unix::fs::symlink(repo.join("src/main.rs"), docs.join("leak.adoc")).unwrap();

        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let err = read(&docs_only, "leak.adoc").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_docs_only_excludes_source_files() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let entries = list(&scope, None).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"intro.adoc"));
        assert!(!paths.contains(&"script.py"));
        assert!(!paths.iter().any(|p| p.ends_with("main.rs")));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_full_repo_includes_source_files() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let entries = list(&scope, None).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"docs/intro.adoc"));
        assert!(paths.contains(&"docs/script.py"));
        assert!(paths.contains(&"src/main.rs"));

        fs::remove_dir_all(&repo).ok();
    }

    /// Regression test: `list_full_repo` used to hardcode `is_dir: false`
    /// on every entry, so a real directory was indistinguishable from a
    /// file in the model's eyes — see `workspace_scanner::
    /// scan_all_entries_with_depth`.
    #[test]
    fn list_files_full_repo_reports_directories() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let entries = list(&scope, None).unwrap();
        let is_dir_of = |p: &str| entries.iter().find(|e| e.path == p).map(|e| e.is_dir);
        assert_eq!(is_dir_of("docs"), Some(true));
        assert_eq!(is_dir_of("docs/intro.adoc"), Some(false));

        fs::remove_dir_all(&repo).ok();
    }

    /// An empty directory has zero files under it — under the old
    /// files-only scan it never appeared in the listing at all (not just
    /// mislabeled, genuinely invisible). Confirms
    /// `scan_all_entries_with_depth` surfaces it.
    #[test]
    fn list_files_full_repo_includes_empty_directory() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(repo.join("empty")).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let entries = list(&scope, None).unwrap();
        let empty = entries.iter().find(|e| e.path == "empty");
        assert_eq!(empty.map(|e| e.is_dir), Some(true));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_full_read_reports_total_lines_and_is_byte_identical() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "intro.adoc", None, None).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "= Intro\n");
                assert_eq!((start_line, end_line, total_lines), (1, 1, 1));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_returns_a_requested_line_range() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("multi.adoc"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "multi.adoc", Some(2), Some(4)).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "two\nthree\nfour\n");
                assert_eq!((start_line, end_line, total_lines), (2, 4, 5));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_clamps_out_of_range_start_and_end_line() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("multi.adoc"), "one\ntwo\nthree\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "multi.adoc", Some(0), Some(9999)).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "one\ntwo\nthree\n");
                assert_eq!((start_line, end_line, total_lines), (1, 3, 3));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_clamps_end_line_below_start_line_up_to_start_line() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("multi.adoc"), "one\ntwo\nthree\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "multi.adoc", Some(3), Some(1)).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "three\n");
                assert_eq!((start_line, end_line, total_lines), (3, 3, 3));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_line_range_on_empty_file_reports_zero_lines() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("empty.adoc"), "").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "empty.adoc", Some(1), Some(5)).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "");
                assert_eq!((start_line, end_line, total_lines), (0, 0, 0));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    /// The key regression test for the `list_docs_only` walk-scoping fix:
    /// without it, `depth` would be measured from `docs_root` instead of
    /// from the requested `path`, silently producing wrong results.
    #[test]
    fn list_files_depth_is_relative_to_requested_subdir_not_root() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("a/b")).unwrap();
        fs::write(docs.join("a/direct.adoc"), "= Direct\n").unwrap();
        fs::write(docs.join("a/b/nested.adoc"), "= Nested\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let entries = list_scoped(&scope, Some("a"), Some(1), None).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"a/direct.adoc"));
        assert!(paths.contains(&"a/b"));
        // depth=1 relative to "a" excludes "a"'s grandchildren.
        assert!(!paths.contains(&"a/b/nested.adoc"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_depth_limits_recursion_in_docs_only() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("a/b")).unwrap();
        fs::write(docs.join("a/one.adoc"), "= One\n").unwrap();
        fs::write(docs.join("a/b/two.adoc"), "= Two\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let shallow = list_scoped(&scope, None, Some(2), None).unwrap();
        let shallow_paths: Vec<&str> = shallow.iter().map(|e| e.path.as_str()).collect();
        assert!(shallow_paths.contains(&"a/one.adoc"));
        assert!(!shallow_paths.contains(&"a/b/two.adoc"));

        let unlimited = list_scoped(&scope, None, None, None).unwrap();
        let unlimited_paths: Vec<&str> = unlimited.iter().map(|e| e.path.as_str()).collect();
        assert!(unlimited_paths.contains(&"a/b/two.adoc"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_depth_limits_recursion_in_full_repo() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(repo.join("src/nested")).unwrap();
        fs::write(repo.join("src/nested/deep.rs"), "fn deep() {}\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let shallow = list_scoped(&scope, None, Some(2), None).unwrap();
        let shallow_paths: Vec<&str> = shallow.iter().map(|e| e.path.as_str()).collect();
        assert!(shallow_paths.contains(&"src/main.rs"));
        assert!(!shallow_paths.iter().any(|p| p.ends_with("deep.rs")));

        let unlimited = list_scoped(&scope, None, None, None).unwrap();
        let unlimited_paths: Vec<&str> = unlimited.iter().map(|e| e.path.as_str()).collect();
        assert!(unlimited_paths.iter().any(|p| p.ends_with("deep.rs")));

        fs::remove_dir_all(&repo).ok();
    }

    /// Regression coverage for a real user report: `listFiles` on an
    /// existing, non-root subdirectory (`path` non-`None`, combined with
    /// `depth`) in Full-repo mode. `resolve_subdir`/`join_relative`/
    /// `ensure_under`/`relative_to` treat a one-segment and a multi-segment
    /// path identically (no depth-dependent logic anywhere in that chain),
    /// so this is expected to behave exactly like the root-path case above
    /// — this test exists to actually pin that down rather than leave it
    /// unverified.
    #[test]
    fn list_files_nested_path_with_depth_in_full_repo_returns_real_contents() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(repo.join("src/nested/deeper")).unwrap();
        fs::write(repo.join("src/nested/one.rs"), "fn one() {}\n").unwrap();
        fs::write(repo.join("src/nested/deeper/two.rs"), "fn two() {}\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let entries = list_scoped(&scope, Some("src/nested"), Some(1), None).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"src/nested/one.rs"));
        assert!(paths.contains(&"src/nested/deeper"));
        assert!(!paths.iter().any(|p| p.ends_with("two.rs")));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_depth_zero_returns_no_descendant_entries() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let entries = list_scoped(&scope, None, Some(0), None).unwrap();
        assert!(entries.is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_pattern_filters_by_basename_across_depths() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(repo.join("src/sub")).unwrap();
        fs::write(repo.join("src/a.java"), "class A {}\n").unwrap();
        fs::write(repo.join("src/sub/b.java"), "class B {}\n").unwrap();
        fs::write(repo.join("src/sub/c.txt"), "not java\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let entries = list_scoped(&scope, Some("src"), None, Some("*.java")).unwrap();
        let mut paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        paths.sort();
        // "src/sub" itself doesn't match "*.java" but is kept regardless —
        // `pattern` scopes which *files* come back, not the directory
        // structure (see `list_files_pattern_keeps_directory_entries` for
        // the same rule in Docs-only mode). This only became observable in
        // Full-repo mode once `list_full_repo` started reporting real
        // directory entries at all.
        assert_eq!(paths, vec!["src/a.java", "src/sub", "src/sub/b.java"]);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_pattern_keeps_directory_entries() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("assets")).unwrap();
        fs::write(docs.join("assets/logo.png"), "not adoc").ok();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // `assets` itself doesn't match "*.adoc", but must still be listed
        // — `pattern` scopes which files come back, not the directory
        // structure.
        let entries = list_scoped(&scope, None, None, Some("*.adoc")).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"assets"));
        assert!(paths.contains(&"intro.adoc"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_invalid_glob_pattern_returns_invalid_pattern_error() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = list_scoped(&scope, None, None, Some("[")).unwrap_err();
        assert!(matches!(err, ToolError::InvalidPattern(_)));

        fs::remove_dir_all(&repo).ok();
    }

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
    fn scope_for_config_defaults_to_both_tools_when_unset() {
        let (repo, docs) = fixture_repo();
        let config = ProjectConfig::new(".");

        let scope = scope_for_config(&repo, &docs, &config);
        assert!(read(&scope, "intro.adoc").is_ok());
        assert!(list(&scope, None).is_ok());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn scope_for_config_honors_a_customized_allowlist() {
        let (repo, docs) = fixture_repo();
        let mut config = ProjectConfig::new(".");
        config.ai_allowed_tools = Some(vec![ToolName::ListFiles]);

        let scope = scope_for_config(&repo, &docs, &config);
        assert!(matches!(
            read(&scope, "intro.adoc").unwrap_err(),
            ToolError::NotAllowed(ToolName::ReadFile)
        ));
        assert!(list(&scope, None).is_ok());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn migrate_plan_tools_into_allowlist_backfills_only_missing_plan_tools() {
        let mut config = ProjectConfig::new(".");
        config.ai_allowed_tools = Some(vec![ToolName::ListFiles, ToolName::ReadPlan]);

        let changed = migrate_plan_tools_into_allowlist(&mut config);

        assert!(changed);
        let list = config.ai_allowed_tools.unwrap();
        assert!(list.contains(&ToolName::CreatePlan));
        assert!(list.contains(&ToolName::UpdatePlan));
        assert_eq!(list.iter().filter(|t| **t == ToolName::ReadPlan).count(), 1);
        assert!(list.contains(&ToolName::UpdatePlanTodo));
        assert!(list.contains(&ToolName::Skill));
        assert!(!list.contains(&ToolName::WriteFile));
    }

    #[test]
    fn migrate_plan_tools_into_allowlist_is_noop_when_unset() {
        let mut config = ProjectConfig::new(".");
        assert!(!migrate_plan_tools_into_allowlist(&mut config));
        assert!(config.ai_allowed_tools.is_none());
    }

    #[test]
    fn migrate_plan_tools_into_allowlist_is_idempotent() {
        let mut config = ProjectConfig::new(".");
        config.ai_allowed_tools = Some(vec![ToolName::ListFiles]);
        assert!(migrate_plan_tools_into_allowlist(&mut config));
        assert!(!migrate_plan_tools_into_allowlist(&mut config));
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
    fn symbol_matches_finds_an_exact_case_insensitive_name() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/UserService.java"),
            "public class UserService {\n    public String getName() { return null; }\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(&repo_index, &scope, "userservice", 10);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].source, MatchSource::Symbol);
        assert!(matches[0].path.ends_with("UserService.java"));
        assert_eq!(matches[0].score, 1.0);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_extracts_tokens_from_natural_language_query() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/CollectNotificationService.java"),
            "public class CollectNotificationService {\n    public void run() {}\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(
            &repo_index,
            &scope,
            "алгоритм формирования списка уведомлений для подачи notifications",
            10,
        );
        assert!(!matches.is_empty());
        assert!(matches.iter().any(|m| m.path.contains("CollectNotificationService")));
        assert!(matches.iter().all(|m| m.source == MatchSource::Symbol));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_finds_multiple_identifiers_in_one_query() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/CollectNotificationService.java"),
            "public class CollectNotificationService {}\n",
        )
        .unwrap();
        fs::write(
            docs.join("getPatentNotifications.adoc"),
            "= getPatentNotifications\n",
        )
        .unwrap();
        // AsciiDoc section may or may not index as a symbol named
        // getPatentNotifications — path match still covers the folder/file.
        fs::create_dir_all(docs.join("getPatentNotifications")).unwrap();
        fs::write(
            docs.join("getPatentNotifications/getPatentNotifications.adoc"),
            "= Method\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(
            &repo_index,
            &scope,
            "CollectNotificationService getPatentNotifications",
            10,
        );
        assert!(matches.iter().any(|m| m.path.contains("CollectNotificationService")));
        assert!(matches.iter().any(|m| m.path.contains("getPatentNotifications")));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_dedupes_symbol_over_path_for_same_file() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/UserService.java"),
            "public class UserService {\n    public String getName() { return null; }\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(&repo_index, &scope, "UserService", 10);
        let path_hits: Vec<_> = matches
            .iter()
            .filter(|m| m.path.ends_with("UserService.java"))
            .collect();
        assert_eq!(path_hits.len(), 1);
        assert_eq!(path_hits[0].score, 1.0);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_is_empty_for_an_unknown_name() {
        let (repo, docs) = fixture_repo();
        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        assert!(symbol_matches(&repo_index, &scope, "NoSuchSymbol", 10).is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_cyrillic_only_query_finds_via_ru_en() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/CollectNotificationService.java"),
            "public class CollectNotificationService {}\n",
        )
        .unwrap();
        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(
            &repo_index,
            &scope,
            "алгоритм формирования списка уведомлений",
            10,
        );
        assert!(
            matches.iter().any(|m| m.path.contains("CollectNotificationService")),
            "RU→EN Notification + stem should find CollectNotificationService"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_stem_finds_notification_service() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/CollectNotificationService.java"),
            "public class CollectNotificationService {}\n",
        )
        .unwrap();
        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(&repo_index, &scope, "notifications", 10);
        assert!(matches.iter().any(|m| m.path.contains("CollectNotificationService")));
        assert!(matches.iter().any(|m| (m.score - 0.95).abs() < 0.01 || m.score >= 0.95));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_excludes_non_doc_symbols_in_docs_only() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/UserService.java"),
            "public class UserService {\n    public String getName() { return null; }\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        assert!(symbol_matches(&repo_index, &scope, "userservice", 10).is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_finds_a_case_insensitive_substring() {
        use crate::domain::chunk_index::ChunkBuildOptions;
        use crate::services::chunk_builder::ChunkBuilder;

        let (repo, docs) = fixture_repo();
        fs::write(repo.join("docs/needle.adoc"), "= Guide\n\nfind the NEEDLE here\n").unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(ChunkBuilder::new().build_all(&repo_index, &ChunkBuildOptions::default()));
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = lexical_matches(&chunk_index, &scope, "needle", 10);
        assert!(!matches.is_empty());
        assert_eq!(matches[0].source, MatchSource::Lexical);
        assert!(matches[0].snippet.to_lowercase().contains("needle"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_tokenizes_natural_language_query() {
        use crate::domain::chunk_index::ChunkBuildOptions;
        use crate::services::chunk_builder::ChunkBuilder;

        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("docs/guide.adoc"),
            "= Guide\n\nHere we describe notifications for patent submit.\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(ChunkBuilder::new().build_all(&repo_index, &ChunkBuildOptions::default()));
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = lexical_matches(
            &chunk_index,
            &scope,
            "алгоритм формирования списка уведомлений notifications",
            10,
        );
        assert!(!matches.is_empty());
        assert!(matches[0].snippet.to_lowercase().contains("notifications"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_is_empty_for_an_empty_query() {
        let (repo, docs) = fixture_repo();
        let chunk_index = ChunkIndex::new();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        assert!(lexical_matches(&chunk_index, &scope, "", 10).is_empty());
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_excludes_non_doc_chunks_in_docs_only() {
        use crate::domain::chunk_index::ChunkBuildOptions;
        use crate::services::chunk_builder::ChunkBuilder;

        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/Needle.java"),
            "public class Needle {\n    // find the NEEDLE here\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(ChunkBuilder::new().build_all(&repo_index, &ChunkBuildOptions::default()));
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        assert!(lexical_matches(&chunk_index, &scope, "needle", 10).is_empty());

        fs::remove_dir_all(&repo).ok();
    }

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
    fn grep_finds_line_hits_under_docs_root_and_rejects_invalid_regex() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("guide.adoc"), "= Guide\ncall Needle.here()\nmore\n").unwrap();
        fs::write(repo.join("src/main.rs"), "fn Needle() {}\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let result = execute_tool(
            &scope,
            ToolCall::Grep(GrepArgs {
                pattern: "Needle".to_string(),
                path: None,
                glob: None,
                case_insensitive: None,
                max_results: None,
            }),
            &EmbeddingDeps::empty(),
            &[],
        )
        .unwrap();
        match result {
            ToolResult::GrepResults { matches, truncated } => {
                assert!(!truncated);
                assert_eq!(matches.len(), 1);
                assert_eq!(matches[0].path, "guide.adoc");
                assert_eq!(matches[0].line, 2);
                assert!(matches[0].text.contains("Needle"));
            }
            other => panic!("expected GrepResults, got {other:?}"),
        }

        let err = execute_tool(
            &scope,
            ToolCall::Grep(GrepArgs {
                pattern: "(unclosed".to_string(),
                path: None,
                glob: None,
                case_insensitive: None,
                max_results: None,
            }),
            &EmbeddingDeps::empty(),
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::InvalidPattern(_)), "got {err:?}");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn grep_truncates_when_max_results_is_hit() {
        let (repo, docs) = fixture_repo();
        let mut body = String::new();
        for i in 0..10 {
            body.push_str(&format!("hit {i}\n"));
        }
        fs::write(docs.join("many.adoc"), body).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let result = execute_tool(
            &scope,
            ToolCall::Grep(GrepArgs {
                pattern: "hit".to_string(),
                path: None,
                glob: None,
                case_insensitive: None,
                max_results: Some(3),
            }),
            &EmbeddingDeps::empty(),
            &[],
        )
        .unwrap();
        match result {
            ToolResult::GrepResults { matches, truncated } => {
                assert!(truncated);
                assert_eq!(matches.len(), 3);
            }
            other => panic!("expected GrepResults, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
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
    fn check_problems_returns_broken_xref_for_all_and_for_one_file() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("broken.adoc"), "xref:nope.adoc[]\n").unwrap();
        fs::write(docs.join("clean.adoc"), "[[ok]]\n= Clean\n").unwrap();

        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps {
            workspace_index,
            ..EmbeddingDeps::empty()
        };

        let all = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: None,
            }),
            &deps,
            &[],
        )
        .unwrap();
        match all {
            ToolResult::CheckResults {
                kind,
                diagnostics,
                truncated,
            } => {
                assert_eq!(kind, CheckKind::Problems);
                assert!(!truncated);
                assert!(
                    diagnostics.iter().any(|d| {
                        d.kind
                            == crate::domain::workspace_index::DiagnosticKind::MissingXrefDocument
                            && d.document.as_str() == "broken.adoc"
                    }),
                    "got: {diagnostics:?}"
                );
                assert!(
                    !diagnostics
                        .iter()
                        .any(|d| d.document.as_str() == "clean.adoc"),
                    "clean file should not appear: {diagnostics:?}"
                );
            }
            other => panic!("expected CheckResults, got {other:?}"),
        }

        let one = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: Some("broken.adoc".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap();
        match one {
            ToolResult::CheckResults { diagnostics, .. } => {
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0].document.as_str(), "broken.adoc");
            }
            other => panic!("expected CheckResults, got {other:?}"),
        }

        let clean = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: Some("clean.adoc".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap();
        match clean {
            ToolResult::CheckResults { diagnostics, .. } => {
                assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
            }
            other => panic!("expected CheckResults, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn check_problems_full_repo_returns_access_relative_document_paths() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("broken.adoc"), "xref:nope.adoc[]\n").unwrap();

        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let deps = EmbeddingDeps {
            workspace_index,
            ..EmbeddingDeps::empty()
        };

        let one = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: Some("docs/broken.adoc".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap();
        match one {
            ToolResult::CheckResults { diagnostics, .. } => {
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0].document.as_str(), "docs/broken.adoc");
            }
            other => panic!("expected CheckResults, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
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
    fn check_problems_rejects_path_escape_under_docs_root() {
        let (repo, docs) = fixture_repo();
        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps {
            workspace_index,
            ..EmbeddingDeps::empty()
        };

        let err = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: Some("../src/main.rs".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)), "got {err:?}");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn check_problems_truncates_when_over_cap() {
        let (repo, docs) = fixture_repo();
        let mut body = String::new();
        for i in 0..(MAX_CHECK_DIAGNOSTICS + 5) {
            body.push_str(&format!("xref:missing{i}.adoc[]\n"));
        }
        fs::write(docs.join("many.adoc"), body).unwrap();

        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps {
            workspace_index,
            ..EmbeddingDeps::empty()
        };

        let result = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: Some("many.adoc".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap();
        match result {
            ToolResult::CheckResults {
                diagnostics,
                truncated,
                ..
            } => {
                assert!(truncated);
                assert_eq!(diagnostics.len(), MAX_CHECK_DIAGNOSTICS);
            }
            other => panic!("expected CheckResults, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    /// Writes a `methodName` folder under `root` that passes every default
    /// standards rule (mirrors `services::standards::tests::write_full_method`).
    fn write_full_standards_method(root: &Path, method: &str) {
        let dir = root.join(method);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{method}.puml")), "@startuml\n@enduml").unwrap();
        let main = format!(
            "= {method}\n:toc:\n\n== Назначение\nЭто достаточно длинное описание метода для прохождения проверки на пятьдесят символов.\n\n== Схема работы\nimage::{method}.puml[]\n\n== Описание входных параметров\n|===\n| Имя | Тип | Обязательный | Описание\n| id | string | да | идентификатор\n|===\n\ninclude::./request.adoc[]\n\n== Описание выходных параметров\n|===\n| Имя | Тип | Обязательный | Описание\n| id | string | да | идентификатор\n|===\n\ninclude::./response.adoc[]\n\n== Алгоритм работы\nШаг 1.\n\n== Обработка ошибок\n404 - не найдено.\n"
        );
        fs::write(dir.join(format!("{method}.adoc")), main).unwrap();
        fs::write(dir.join("request.adoc"), "${HOST}/api/x\ncurl example").unwrap();
        fs::write(dir.join("response.adoc"), "{}").unwrap();
    }

    #[test]
    fn check_standards_whole_project_reports_per_folder_results() {
        let (repo, docs) = fixture_repo();
        write_full_standards_method(&docs, "getUser");
        fs::create_dir_all(docs.join("createUser")).unwrap();
        fs::write(docs.join("createUser").join("createUser.adoc"), "= createUser").unwrap();

        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps { workspace_index, ..EmbeddingDeps::empty() };

        let result = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs { kind: CheckKind::Standards, path: None }),
            &deps,
            &[],
        )
        .unwrap();
        match result {
            ToolResult::StandardsChecked { report, truncated } => {
                assert!(!truncated);
                assert_eq!(report.folders.len(), 2);
                // Failing folder sorted first.
                assert_eq!(report.folders[0].method_name, "createUser");
                assert!(!report.folders[0].passed);
                assert_eq!(report.folders[1].method_name, "getUser");
                assert!(report.folders[1].passed, "{:?}", report.folders[1]);
                assert!(!report.overall_passed);
            }
            other => panic!("expected StandardsChecked, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn check_standards_scopes_to_method_folder_via_file_path() {
        let (repo, docs) = fixture_repo();
        write_full_standards_method(&docs, "getUser");
        fs::create_dir_all(docs.join("createUser")).unwrap();
        fs::write(docs.join("createUser").join("createUser.adoc"), "= createUser").unwrap();

        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps { workspace_index, ..EmbeddingDeps::empty() };

        // Points at a *file* inside getUser/ — should resolve to that
        // folder only, not the whole docs root.
        let result = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Standards,
                path: Some("getUser/getUser.adoc".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap();
        match result {
            ToolResult::StandardsChecked { report, .. } => {
                assert_eq!(report.folders.len(), 1);
                assert_eq!(report.folders[0].method_name, "getUser");
                assert!(report.folders[0].passed);
            }
            other => panic!("expected StandardsChecked, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn check_standards_rejects_path_escape_under_docs_root() {
        let (repo, docs) = fixture_repo();
        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps { workspace_index, ..EmbeddingDeps::empty() };

        let err = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Standards,
                path: Some("../src/main.rs".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)), "got {err:?}");

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
    fn git_diff_and_git_blame_reject_paths_outside_docs_root_in_docs_only() {
        let (repo, docs) = fixture_repo();
        // Real git repo so the tools get past open_repo — containment must
        // still fail before any blob read.
        {
            let git_repo = git2::Repository::init(&repo).unwrap();
            let mut config = git_repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps::empty();

        let err = execute_tool(
            &scope,
            ToolCall::GitDiff(GitDiffArgs {
                path: "../src/main.rs".to_string(),
                scope: None,
                commit: None,
            }),
            &deps,
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)), "got {err:?}");

        let err = execute_tool(
            &scope,
            ToolCall::GitBlame(GitBlameArgs {
                path: "../src/main.rs".to_string(),
                start_line: None,
                end_line: None,
            }),
            &deps,
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)), "got {err:?}");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn git_diff_returns_unified_diff_for_unstaged_change_under_docs_root() {
        let (repo, docs) = fixture_repo();
        {
            let git_repo = git2::Repository::init(&repo).unwrap();
            let mut config = git_repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
            // Commit the docs file, then dirty the worktree.
            let mut index = git_repo.index().unwrap();
            index.add_path(Path::new("docs/intro.adoc")).unwrap();
            index.write().unwrap();
            let tree_oid = index.write_tree().unwrap();
            let tree = git_repo.find_tree(tree_oid).unwrap();
            let sig = git2::Signature::now("Test", "test@test.com").unwrap();
            git_repo
                .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap();
        }
        fs::write(docs.join("intro.adoc"), "= Intro\nchanged\n").unwrap();

        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let result = execute_tool(
            &scope,
            ToolCall::GitDiff(GitDiffArgs {
                path: "intro.adoc".to_string(),
                scope: Some("unstaged".to_string()),
                commit: None,
            }),
            &EmbeddingDeps::empty(),
            &[],
        )
        .unwrap();
        match result {
            ToolResult::GitDiff { path, label, diff, is_binary } => {
                assert_eq!(path, "intro.adoc");
                assert!(label.contains("Working tree") || label.contains("Index") || label.contains("HEAD"));
                assert!(!is_binary);
                assert!(diff.lines_added > 0 || diff.unified_diff.contains('+'));
            }
            other => panic!("expected GitDiff, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
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
    fn execute_tool_skill_search_does_not_return_disabled() {
        crate::infra::settings_store::test_support::with_temp_home(|| {
            crate::services::agent_skills::set_skill_enabled(
                crate::domain::agent_skills::SkillSource::Bundled,
                "rest-endpoint-docs",
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
                    assert!(!hits.matches.iter().any(|m| m.name == "rest-endpoint-docs"));
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

    fn todo_write(scope: &ToolScope, todos: &[Task], titles: &[&str]) -> Result<Vec<Task>, ToolError> {
        match execute_tool(
            scope,
            ToolCall::TodoWrite(TodoWriteArgs {
                titles: titles.iter().map(|s| s.to_string()).collect(),
            }),
            &EmbeddingDeps::empty(),
            todos,
        )? {
            ToolResult::TodoWritten(list) => Ok(list),
            other => panic!("expected ToolResult::TodoWritten, got {other:?}"),
        }
    }

    fn todo_update(
        scope: &ToolScope,
        todos: &[Task],
        id: &str,
        status: TodoUpdateStatus,
        note: Option<&str>,
    ) -> Result<Vec<Task>, ToolError> {
        match execute_tool(
            scope,
            ToolCall::TodoUpdate(TodoUpdateArgs {
                id: id.to_string(),
                status,
                note: note.map(str::to_string),
            }),
            &EmbeddingDeps::empty(),
            todos,
        )? {
            ToolResult::TodoUpdated(list) => Ok(list),
            other => panic!("expected ToolResult::TodoUpdated, got {other:?}"),
        }
    }

    #[test]
    fn todo_write_on_empty_list_marks_first_task_in_progress_rest_pending() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let list = todo_write(&scope, &[], &["Найти контроллер", "Найти сервис", "Реализовать endpoint"]).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].status, TodoStatus::InProgress);
        assert_eq!(list[1].status, TodoStatus::Pending);
        assert_eq!(list[2].status, TodoStatus::Pending);
        assert_eq!(list[0].id, "t1");
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn todo_write_appends_to_an_existing_list_without_disturbing_in_progress() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let list = todo_write(&scope, &[], &["A", "B"]).unwrap();
        let list = todo_write(&scope, &list, &["C"]).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].status, TodoStatus::InProgress);
        assert_eq!(list[2].id, "t3");
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn todo_write_beyond_max_tasks_fails_without_mutating() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let titles: Vec<&str> = (0..20).map(|_| "Задача").collect();
        let list = todo_write(&scope, &[], &titles).unwrap();
        assert_eq!(list.len(), 20);
        let err = todo_write(&scope, &list, &["Ещё одна"]).unwrap_err();
        assert!(matches!(
            err,
            ToolError::TooManyTasks { current: 20, adding: 1, max: 20 }
        ));
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn todo_update_completing_current_task_auto_promotes_next_pending() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let list = todo_write(&scope, &[], &["A", "B"]).unwrap();
        let list = todo_update(&scope, &list, "t1", TodoUpdateStatus::Completed, Some("done")).unwrap();
        assert_eq!(list[0].status, TodoStatus::Completed);
        assert_eq!(list[0].note.as_deref(), Some("done"));
        assert_eq!(list[1].status, TodoStatus::InProgress);
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn todo_update_cancelling_current_task_auto_promotes_next_pending() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let list = todo_write(&scope, &[], &["A", "B"]).unwrap();
        let list = todo_update(&scope, &list, "t1", TodoUpdateStatus::Cancelled, Some("not needed")).unwrap();
        assert_eq!(list[0].status, TodoStatus::Cancelled);
        assert_eq!(list[1].status, TodoStatus::InProgress);
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn todo_update_on_last_remaining_task_leaves_nothing_in_progress() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let list = todo_write(&scope, &[], &["A"]).unwrap();
        let list = todo_update(&scope, &list, "t1", TodoUpdateStatus::Completed, None).unwrap();
        assert!(list.iter().all(|t| t.status != TodoStatus::InProgress));
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn todo_update_unknown_id_fails() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let list = todo_write(&scope, &[], &["A"]).unwrap();
        let err = todo_update(&scope, &list, "t99", TodoUpdateStatus::Completed, None).unwrap_err();
        assert!(matches!(err, ToolError::TaskNotFound(id) if id == "t99"));
        fs::remove_dir_all(&repo).ok();
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
