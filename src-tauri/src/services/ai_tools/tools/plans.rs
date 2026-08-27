//! The four plan tools — create, update, read, and per-todo status.
//!
//! Unlike `todo`, a plan outlives the turn: it is persisted by
//! `services::plans` and can be reopened from the UI later.

use crate::domain::ai_tools::{CreatePlanArgs, ReadPlanArgs, ToolError, ToolResult, UpdatePlanArgs, UpdatePlanTodoArgs};
use crate::domain::llm::LlmToolDefinition;

pub(super) fn create_plan(args: CreatePlanArgs) -> Result<ToolResult, ToolError> {
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

pub(super) fn update_plan(args: UpdatePlanArgs) -> Result<ToolResult, ToolError> {
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

pub(super) fn read_plan(args: ReadPlanArgs) -> Result<ToolResult, ToolError> {
    let record = crate::services::plans::read_plan(&args.plan_id)?;
    Ok(ToolResult::PlanRead {
        plan_id: record.id,
        name: record.name,
        overview: record.overview,
        plan: record.plan,
        todos: record.todos,
    })
}

pub(super) fn update_plan_todo(args: UpdatePlanTodoArgs) -> Result<ToolResult, ToolError> {
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

/// The `createPlan` schema the model sees.
pub(super) fn create_definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "createPlan".to_string(),
        description:
            "Create a persisted work plan as the final deliverable of Plan mode. Call this AFTER research with read-only tools — do not dump the full plan as chat prose; the UI shows a plan card from this tool result. `name` is a short 3–4 word title; `overview` is 1–2 sentences; `plan` is the full markdown body (first line MUST be a `# Title` heading) and MUST be self-contained: a later Agent turn executes from this artifact alone, without the planning conversation; `todos` is an array of at least 2 concrete checklist items with stable slug `id`s (e.g. \"setup-auth\") and imperative `content`. Returns `planId` — remember it for later `updatePlan` calls in this session. After success, reply with a brief 1–3 sentence summary only; the card has «Открыть» / «Начать» buttons."
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
                    "description": "Full markdown plan body; first line must be `# Title`. Must be self-contained for execution without the planning chat (goal, research digest, files, steps with acceptance criteria, rejected alternatives)."
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
        }
}

/// The `updatePlan` schema the model sees.
pub(super) fn update_definition() -> LlmToolDefinition {
    LlmToolDefinition {
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
        }
}

/// The `readPlan` schema the model sees.
pub(super) fn read_definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "readPlan".to_string(),
        description:
            "Load a persisted plan by `planId` — full markdown body and current todo statuses. In Agent mode the live snapshot is already injected each turn; call this only to refresh after an external change. In Plan mode, use it to refresh context before `updatePlan`."
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
        }
}

/// The `updatePlanTodo` schema the model sees.
pub(super) fn update_todo_definition() -> LlmToolDefinition {
    LlmToolDefinition {
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
        }
}
