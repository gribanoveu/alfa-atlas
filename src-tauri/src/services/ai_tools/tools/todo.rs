//! `todo` — the model's own task list for a multi-step turn.
//!
//! One wire-level tool that `parse` fans out into write and update
//! variants. The list is handed in and returned rather than stored here:
//! it lives in the chat turn, not on disk.

use crate::domain::ai_tools::{Task, TodoStatus, TodoUpdateArgs, TodoWriteArgs, ToolError};
use crate::domain::llm::LlmToolDefinition;

/// Hard cap on total tasks in a todo list, enforced by `todo_write` — a
/// `write` that would exceed this fails outright (see
/// `ToolError::TooManyTasks`) rather than silently truncating.
pub(super) const MAX_TODO_TASKS: usize = 20;

pub(super) fn todo_write(todos: &[Task], args: TodoWriteArgs) -> Result<Vec<Task>, ToolError> {
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

/// Completes or cancels one task. An absent `id` means the active one.
///
/// The default exists because naming an id is a step that can be got wrong
/// silently: in the transcript that prompted it, the model closed `t6` while
/// meaning `t5`, and the checklist ended the turn claiming a finished step
/// was still outstanding. "The task I am on" needs no id, and that is what
/// almost every update means.
pub(super) fn todo_update(todos: &[Task], args: TodoUpdateArgs) -> Result<Vec<Task>, ToolError> {
    let mut updated = todos.to_vec();
    let target = match args.id.clone() {
        Some(id) => id,
        None => todos
            .iter()
            .find(|t| t.status == TodoStatus::InProgress)
            .map(|t| t.id.clone())
            .ok_or_else(|| ToolError::TaskNotFound {
                id: String::new(),
                available: Some(todos.iter().map(|t| t.id.clone()).collect()),
            })?,
    };
    let task = updated
        .iter_mut()
        .find(|t| t.id == target)
        .ok_or_else(|| ToolError::TaskNotFound {
            id: target.clone(),
            available: Some(todos.iter().map(|t| t.id.clone()).collect()),
        })?;
    task.status = args.status.into();
    if let Some(note) = args.note {
        task.note = Some(note);
    }
    Ok(enforce_todo_invariant(updated))
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
pub(super) fn enforce_todo_invariant(mut tasks: Vec<Task>) -> Vec<Task> {
    let has_in_progress = tasks.iter().any(|t| t.status == TodoStatus::InProgress);
    if !has_in_progress {
        if let Some(next) = tasks.iter_mut().find(|t| t.status == TodoStatus::Pending) {
            next.status = TodoStatus::InProgress;
        }
    }
    tasks
}

/// The `todo` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "todo".to_string(),
        description: "Manage your working task checklist for a multi-step request (3+ steps). One tool, two operations selected via `op`. `op: \"write\"` adds new task titles (`tasks`, an array of short imperative strings, 3-7 words each) to the end of the checklist; the runtime assigns each an id and, if the checklist was empty before this call, marks the first of the new tasks in_progress automatically (the rest start pending) — calling `write` again later appends more titles to the end, it never replaces or clears the existing list. `op: \"update\"` changes one existing task — the active one when `id` is omitted, or the task named by `id` exactly as shown in your current checklist — to `status: \"completed\"` or `status: \"cancelled\"` (optionally with a short `note`: a brief result for a completed task, or the reason for a cancelled one) — these are the ONLY two status values you may set; you can never set `pending` or `in_progress` yourself, the runtime handles those transitions automatically, including auto-activating the next pending task the instant the current one is completed or cancelled. There is no `read` operation: your current checklist, with the active task marked, is always shown to you at the top of your context — never call this tool just to see the list. Do not use this tool for a task with only 1-2 steps, that is a wasted call. At most 20 tasks total in one checklist."
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
                    "description": "Only for op: \"update\". The id of the task to change (e.g. \"t2\"), exactly as shown in your current checklist. Omit it to change the active task — that is what you want whenever you are finishing the step you are on, and it cannot name the wrong one. Ignored for op: \"write\"."
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
        }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::domain::ai_access::AiAccessMode;
    use crate::domain::ai_tools::{
    Task, TodoStatus, TodoUpdateArgs, TodoUpdateStatus, TodoWriteArgs, ToolCall, ToolError,
    ToolResult, ToolScope,
};
    use crate::services::ai_tools::testing::*;
    use crate::services::ai_tools::{EmbeddingDeps, execute_tool};

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
                id: Some(id.to_string()),
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

    /// The same call with `id` omitted — targets whatever is active.
    fn update_active(
        scope: &ToolScope,
        todos: &[Task],
        status: TodoUpdateStatus,
    ) -> Result<Vec<Task>, ToolError> {
        match execute_tool(
            scope,
            ToolCall::TodoUpdate(TodoUpdateArgs { id: None, status, note: None }),
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
    fn an_update_without_an_id_closes_the_active_task() {
        // The default that removes a whole class of silent mistake: naming
        // the wrong existing id closes the wrong row, and the checklist ends
        // the turn disagreeing with the work.
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let list = todo_write(&scope, &[], &["Первая", "Вторая", "Третья"]).unwrap();
        assert_eq!(list[0].status, TodoStatus::InProgress);

        let list = update_active(&scope, &list, TodoUpdateStatus::Completed).unwrap();
        assert_eq!(list[0].status, TodoStatus::Completed);
        assert_eq!(list[1].status, TodoStatus::InProgress, "next one activates as usual");

        let list = update_active(&scope, &list, TodoUpdateStatus::Completed).unwrap();
        assert_eq!(list[1].status, TodoStatus::Completed);
        assert_eq!(list[2].status, TodoStatus::InProgress);
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn an_update_without_an_id_fails_when_nothing_is_active() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let err = update_active(&scope, &[], TodoUpdateStatus::Completed).unwrap_err();
        assert!(matches!(err, ToolError::TaskNotFound { .. }));
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
        assert!(matches!(&err, ToolError::TaskNotFound { id, .. } if id == "t99"));
        // The message has to name the way out — a bare "no task with id"
        // is a dead end the model just abandons.
        assert!(err.to_string().contains("current ids: t1"), "got {err}");
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn todo_update_on_an_empty_checklist_says_to_write_one_first() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        // Exactly what happened in the transcript: `op: "update", id: "t1"`
        // with nothing ever written.
        let err = todo_update(&scope, &[], "t1", TodoUpdateStatus::Completed, None).unwrap_err();
        assert!(err.to_string().contains("the checklist is empty"), "got {err}");
        fs::remove_dir_all(&repo).ok();
    }
}
