//! Application-layer orchestration for work plans — resolve the
//! repository-keyed storage directory, create/update/read/delete plans.

use std::collections::HashSet;

use uuid::Uuid;

use crate::domain::plan::{
    enforce_plan_todo_invariant, PlanError, PlanRecord, PlanSummary, PlanTodo, PlanTodoStatus,
    PlanTodoUpdateStatus,
};
use crate::infra::plan_store;
use crate::services::repository_scope;

/// The repository-keyed storage folder for the open project. Identity
/// resolution itself lives in `services::repository_scope`, which owns the
/// one copy of it (it can mint and persist a `local_repository_id`, so it
/// must not be reimplemented per store).
fn open_repo_id() -> Result<(String, String), PlanError> {
    repository_scope::open_repository().map_err(|e| PlanError::Project(e.to_string()))
}

/// Create a new plan from model-supplied fields. Assigns a UUID id and
/// stamps timestamps; todos start pending with the first promoted to
/// inProgress.
pub fn create_plan(
    name: String,
    overview: String,
    plan: String,
    todos: Vec<(String, String)>,
    chat_id: Option<String>,
) -> Result<PlanRecord, PlanError> {
    validate_create(&name, &overview, &plan, &todos)?;
    let (repo_id, repo_root) = open_repo_id()?;

    let mut seen = HashSet::new();
    let plan_todos: Vec<PlanTodo> = todos
        .into_iter()
        .map(|(id, content)| {
            if id.trim().is_empty() {
                return Err(PlanError::Invalid("todo id must be non-empty".into()));
            }
            if !seen.insert(id.clone()) {
                return Err(PlanError::Invalid(format!("duplicate todo id: {id}")));
            }
            Ok(PlanTodo {
                id,
                content,
                status: PlanTodoStatus::Pending,
                note: None,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let record = plan_store::stamp_new(PlanRecord {
        id: Uuid::new_v4().to_string(),
        name: name.trim().to_string(),
        overview: overview.trim().to_string(),
        plan,
        todos: enforce_plan_todo_invariant(plan_todos),
        created_at_ms: 0,
        updated_at_ms: 0,
        chat_id,
        repo_root: Some(repo_root),
    });
    plan_store::save(&repo_id, &record)?;
    Ok(record)
}

fn validate_create(
    name: &str,
    overview: &str,
    plan: &str,
    todos: &[(String, String)],
) -> Result<(), PlanError> {
    if name.trim().is_empty() {
        return Err(PlanError::Invalid("name must be non-empty".into()));
    }
    if overview.trim().is_empty() {
        return Err(PlanError::Invalid("overview must be non-empty".into()));
    }
    if plan.trim().is_empty() {
        return Err(PlanError::Invalid("plan markdown must be non-empty".into()));
    }
    if todos.len() < 2 {
        return Err(PlanError::Invalid(
            "todos must have at least 2 items for a non-trivial plan".into(),
        ));
    }
    Ok(())
}

/// Patch an existing plan. `None` fields leave the previous value.
/// When `todos` is `Some`, it fully replaces the checklist (statuses reset
/// to pending + promote first), preserving notes only if the same id is
/// reused is intentionally not done — a rewrite is a clean slate.
pub fn update_plan(
    plan_id: &str,
    name: Option<String>,
    overview: Option<String>,
    plan: Option<String>,
    todos: Option<Vec<(String, String)>>,
) -> Result<PlanRecord, PlanError> {
    let (repo_id, _) = open_repo_id()?;
    let mut record = plan_store::get(&repo_id, plan_id)?;

    if let Some(n) = name {
        if n.trim().is_empty() {
            return Err(PlanError::Invalid("name must be non-empty".into()));
        }
        record.name = n.trim().to_string();
    }
    if let Some(o) = overview {
        if o.trim().is_empty() {
            return Err(PlanError::Invalid("overview must be non-empty".into()));
        }
        record.overview = o.trim().to_string();
    }
    if let Some(p) = plan {
        if p.trim().is_empty() {
            return Err(PlanError::Invalid("plan markdown must be non-empty".into()));
        }
        record.plan = p;
    }
    if let Some(todos) = todos {
        if todos.len() < 2 {
            return Err(PlanError::Invalid(
                "todos must have at least 2 items when replacing the checklist".into(),
            ));
        }
        let mut seen = HashSet::new();
        let plan_todos: Vec<PlanTodo> = todos
            .into_iter()
            .map(|(id, content)| {
                if id.trim().is_empty() {
                    return Err(PlanError::Invalid("todo id must be non-empty".into()));
                }
                if !seen.insert(id.clone()) {
                    return Err(PlanError::Invalid(format!("duplicate todo id: {id}")));
                }
                Ok(PlanTodo {
                    id,
                    content,
                    status: PlanTodoStatus::Pending,
                    note: None,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        record.todos = enforce_plan_todo_invariant(plan_todos);
    }

    record = plan_store::stamp_updated(record);
    plan_store::save(&repo_id, &record)?;
    Ok(record)
}

pub fn read_plan(plan_id: &str) -> Result<PlanRecord, PlanError> {
    let (repo_id, _) = open_repo_id()?;
    plan_store::get(&repo_id, plan_id)
}

pub fn update_plan_todo(
    plan_id: &str,
    todo_id: &str,
    status: PlanTodoUpdateStatus,
    note: Option<String>,
) -> Result<PlanRecord, PlanError> {
    let (repo_id, _) = open_repo_id()?;
    let mut record = plan_store::get(&repo_id, plan_id)?;
    let todo = record
        .todos
        .iter_mut()
        .find(|t| t.id == todo_id)
        .ok_or_else(|| PlanError::TodoNotFound(todo_id.to_string()))?;
    todo.status = status.into();
    if let Some(n) = note {
        todo.note = Some(n);
    }
    record.todos = enforce_plan_todo_invariant(record.todos);
    record = plan_store::stamp_updated(record);
    plan_store::save(&repo_id, &record)?;
    Ok(record)
}

pub fn list_plans() -> Result<Vec<PlanSummary>, PlanError> {
    let (repo_id, _) = open_repo_id()?;
    plan_store::list(&repo_id)
}

pub fn get_plan(plan_id: &str) -> Result<PlanRecord, PlanError> {
    read_plan(plan_id)
}

pub fn delete_plan(plan_id: &str) -> Result<(), PlanError> {
    let (repo_id, _) = open_repo_id()?;
    plan_store::delete(&repo_id, plan_id)
}

/// Used by Settings → Paths; does not require an open project.
pub fn plans_root_path() -> Result<std::path::PathBuf, PlanError> {
    Ok(crate::infra::settings_store::settings_dir()?.join("plans"))
}