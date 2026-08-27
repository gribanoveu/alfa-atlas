//! Persisted work plans produced by Plan-mode `createPlan` / `updatePlan`.
//! Stored under `~/.atlas/plans/{repository_id}/{plan_id}.json` — same
//! repository-identity keying as the embeddings cache.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Status of one step in a plan's checklist — same four values as
/// `domain::ai_tools::TodoStatus`, kept as its own type so the plan store
/// does not depend on the chat-scoped `Task` shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanTodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

/// One checklist item on a plan. `id` is model-supplied (stable slug like
/// `"setup-auth"`), unlike chat `Task` ids which are runtime-generated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanTodo {
    pub id: String,
    pub content: String,
    pub status: PlanTodoStatus,
    #[serde(default)]
    pub note: Option<String>,
}

/// Full on-disk plan record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanRecord {
    pub id: String,
    pub name: String,
    pub overview: String,
    /// Markdown body; first line should be `# Title`.
    pub plan: String,
    pub todos: Vec<PlanTodo>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub chat_id: Option<String>,
    #[serde(default)]
    pub repo_root: Option<String>,
}

/// Lightweight row for listing plans without loading full markdown bodies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub id: String,
    pub name: String,
    pub overview: String,
    pub todo_total: u32,
    pub todo_completed: u32,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl PlanRecord {
    pub fn to_summary(&self) -> PlanSummary {
        let todo_total = self
            .todos
            .iter()
            .filter(|t| t.status != PlanTodoStatus::Cancelled)
            .count() as u32;
        let todo_completed = self
            .todos
            .iter()
            .filter(|t| t.status == PlanTodoStatus::Completed)
            .count() as u32;
        PlanSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            overview: self.overview.clone(),
            todo_total,
            todo_completed,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

/// Status values the model may set via `updatePlanTodo` — excludes
/// pending/inProgress (runtime-managed), same pattern as `TodoUpdateStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanTodoUpdateStatus {
    Completed,
    Cancelled,
}

impl From<PlanTodoUpdateStatus> for PlanTodoStatus {
    fn from(s: PlanTodoUpdateStatus) -> Self {
        match s {
            PlanTodoUpdateStatus::Completed => PlanTodoStatus::Completed,
            PlanTodoUpdateStatus::Cancelled => PlanTodoStatus::Cancelled,
        }
    }
}

#[derive(Debug, Error)]
pub enum PlanError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("settings error: {0}")]
    Settings(#[from] crate::domain::settings::SettingsError),
    #[error("project error: {0}")]
    Project(String),
    #[error("plan not found: {0}")]
    NotFound(String),
    #[error("invalid plan: {0}")]
    Invalid(String),
    #[error("no todo with id: {0}")]
    TodoNotFound(String),
}

/// At most one `InProgress` todo; when none and a `Pending` remains, promote
/// the first pending — mirrors `services::ai_tools::tools::todo::enforce_todo_invariant`.
pub fn enforce_plan_todo_invariant(mut todos: Vec<PlanTodo>) -> Vec<PlanTodo> {
    let has_in_progress = todos
        .iter()
        .any(|t| t.status == PlanTodoStatus::InProgress);
    if !has_in_progress {
        if let Some(next) = todos
            .iter_mut()
            .find(|t| t.status == PlanTodoStatus::Pending)
        {
            next.status = PlanTodoStatus::InProgress;
        }
    }
    todos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforce_promotes_first_pending_when_none_in_progress() {
        let todos = enforce_plan_todo_invariant(vec![
            PlanTodo {
                id: "a".into(),
                content: "A".into(),
                status: PlanTodoStatus::Pending,
                note: None,
            },
            PlanTodo {
                id: "b".into(),
                content: "B".into(),
                status: PlanTodoStatus::Pending,
                note: None,
            },
        ]);
        assert_eq!(todos[0].status, PlanTodoStatus::InProgress);
        assert_eq!(todos[1].status, PlanTodoStatus::Pending);
    }

    #[test]
    fn summary_excludes_cancelled_from_total() {
        let record = PlanRecord {
            id: "p1".into(),
            name: "Test".into(),
            overview: "o".into(),
            plan: "# T".into(),
            todos: vec![
                PlanTodo {
                    id: "a".into(),
                    content: "A".into(),
                    status: PlanTodoStatus::Completed,
                    note: None,
                },
                PlanTodo {
                    id: "b".into(),
                    content: "B".into(),
                    status: PlanTodoStatus::Cancelled,
                    note: None,
                },
                PlanTodo {
                    id: "c".into(),
                    content: "C".into(),
                    status: PlanTodoStatus::Pending,
                    note: None,
                },
            ],
            created_at_ms: 0,
            updated_at_ms: 0,
            chat_id: None,
            repo_root: None,
        };
        let s = record.to_summary();
        assert_eq!(s.todo_total, 2);
        assert_eq!(s.todo_completed, 1);
    }
}
