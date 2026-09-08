//! The three tools that change the conversation rather than the project:
//! widening access to the whole repository, switching task mode, and
//! asking the user a question mid-turn.
//!
//! Only `requestFullRepoAccess` mutates anything. A mode switch is a pure
//! acknowledgement the frontend reacts to — `ConversationMode` is never
//! persisted server-side. `askUser` never executes at all: a well-formed
//! pause is answered through `llm_chat_stream_resume`, so reaching its
//! executor means the call was malformed.

use crate::domain::llm::LlmToolDefinition;

/// When an approved `requestModeSwitch` actually changes anything — echoed
/// back in `ToolResult::ModeSwitchRequested` and stated in the schema below,
/// so the model reads the same rule in both places. The backend pins
/// `ConversationMode` for the whole turn (`services::llm_chat`'s `LoopCtx`),
/// and the frontend flushes the new mode onto the picker only once that turn
/// ends (`AssistantConversation`'s `pendingModeSwitchRef`).
pub(super) const MODE_SWITCH_APPLIES_FROM: &str = "nextUserMessage";

/// The `requestFullRepoAccess` schema the model sees.
pub(super) fn full_repo_access_definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "requestFullRepoAccess".to_string(),
        description:
            "Request escalating from docs-only to full-repo access when repository access beyond documentation is genuinely needed to answer the user's request — including a plain question about source code, which does NOT require a mode switch. Requires a stated reason, and always requires explicit user approval. Read the outcome off the tool result, never guess it: approval returns an object with the new access mode and takes effect immediately, for the rest of this same turn; denial returns the text \"Отклонено пользователем\". Do not call this speculatively or repeatedly, and never a second time once access is already full-repo — that call is rejected."
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
        }
}

/// The `requestDependencySources` schema the model sees.
pub(super) fn dependency_sources_definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "requestDependencySources".to_string(),
        description:
            "Ask the user to connect this project's dependency sources — the code of the libraries it depends on — so you can read it. Call this when answering needs the implementation of a class, function or module that is NOT in this repository: an imported library type, a framework behaviour the user is asking about, a stack frame from a dependency. Do NOT call it for code that lives in the repository, and do not call it speculatively or a second time once the sources are connected. Requires a stated reason and always requires explicit user approval. Read the outcome off the tool result, never guess it: approval returns the connected root names plus a summary and takes effect immediately, for the rest of this same turn — search under those roots with grep (path \"@deps/<name>\") and read files with readFile. Denial returns the text \"Отклонено пользователем\"; an error means the project has no connectable sources, in which case say so plainly instead of looking for files that are not there. Note that only what a build actually downloaded can be connected, so some dependencies may still be unavailable afterwards."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Which dependency's code is needed, and what for."
                }
            },
            "required": ["reason"]
        }),
        }
}

/// The `requestModeSwitch` schema the model sees.
pub(super) fn mode_switch_definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "requestModeSwitch".to_string(),
        description:
            "Request switching the conversation to a different mode (\"agent\", \"plan\", or \"question\") when the current mode structurally cannot do what the user is asking. In Plan mode, when asked to actually implement/apply something: request \"agent\". In Question mode, when a request needs a multi-step plan: request \"plan\"; when it needs actual file changes: request \"agent\". In Agent mode, when the request is really just a question with no changes needed: request \"question\"; when it clearly needs a plan drafted first: request \"plan\". Never request a switch merely to read files outside the documentation root — that is what requestFullRepoAccess is for, and it works in every mode. Requires a stated reason, and always requires explicit user approval. Read the outcome off the tool result, never guess it: approval returns an object with \"approved\": true, denial returns the text \"Отклонено пользователем\". Do not call this speculatively; only when the current mode is genuinely the wrong fit for the request. An approved switch does not change the toolset mid-turn: the new mode applies starting with the next user message — after approval, confirm briefly and stop; do not attempt tools that only the new mode would allow."
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
        }
}

/// The `askUser` schema the model sees.
pub(super) fn ask_user_definition() -> LlmToolDefinition {
    LlmToolDefinition {
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
        }
}
