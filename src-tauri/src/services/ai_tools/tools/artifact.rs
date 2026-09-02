//! The two artifact tools: asking the user to fill one in, and reading one
//! back later.
//!
//! `requestArtifact` never executes here. Like `askUser`, a well-formed
//! call pauses the round and is resolved from the user's decision in
//! `services::llm_chat`, so reaching this module's executor means the call
//! arrived some other way (a bare `ai_execute_tool`, say) with no user
//! answer attached.
//!
//! `artifact` is an ordinary read: one wire tool, two operations, dispatched
//! on `op` the same way `todo` is.

use crate::domain::ai_tools::{
    ArtifactCreateArgs, ArtifactReadArgs, ArtifactUpdateArgs, ToolError, ToolResult,
};
use crate::domain::artifact::ArtifactRecord;
use crate::domain::artifact_render;
use crate::domain::llm::LlmToolDefinition;

/// Pairs a record with its AsciiDoc rendering — the shape both
/// `requestArtifact`'s resume path and `artifact read` hand the model, so
/// the model never has to reconstruct tables from the raw spec itself.
pub fn artifact_result(record: ArtifactRecord) -> ToolResult {
    let rendered = artifact_render::render(&record.content);
    ToolResult::Artifact {
        artifact: record,
        rendered,
    }
}

pub(super) fn artifact_list() -> Result<ToolResult, ToolError> {
    let artifacts = crate::services::artifacts::list()?;
    Ok(ToolResult::ArtifactList { artifacts })
}

pub(super) fn artifact_read(args: ArtifactReadArgs) -> Result<ToolResult, ToolError> {
    let record = crate::services::artifacts::get(&args.id)?;
    Ok(artifact_result(record))
}

/// The assistant writing an artifact itself. Rejected for kinds the user
/// owns — see `ArtifactKind::is_agent_authored`.
pub(super) fn artifact_create(args: ArtifactCreateArgs) -> Result<ToolResult, ToolError> {
    let record =
        crate::services::artifacts::create_agent(args.kind, args.title, args.content, None)?;
    Ok(artifact_result(record))
}

pub(super) fn artifact_update(args: ArtifactUpdateArgs) -> Result<ToolResult, ToolError> {
    let record =
        crate::services::artifacts::update_agent(&args.id, args.title, args.content)?;
    Ok(artifact_result(record))
}

/// The `requestArtifact` schema the model sees.
pub(super) fn request_definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "requestArtifact".to_string(),
        description:
            "Ask the user to fill in a structured document (an \"artifact\") in a visual builder, and wait for it. Use when the documentation you are writing needs concrete facts that simply are not in the repository — above all the request/response shape of a REST method: parameters, formats, obligation, descriptions, example payloads, error codes. Prefer this over inventing a plausible table, and over `askUser` (which is for a small blocking choice between a few options, not for collecting dozens of fields). Call it alone in its own tool round. The user may also decline with \"заполню позже\", in which case continue without it and say what is still missing. Once the artifact comes back you receive both the structured data and ready-made AsciiDoc — input/output parameter tables, a curl example, response examples, an error table — already matching the REST method template, so paste those rather than re-deriving them. Artifacts are saved permanently and can be re-read in any later conversation with the `artifact` tool."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["httpRequest"],
                    "description": "Which builder to open. \"httpRequest\" designs one HTTP endpoint: method, URL, path/query/header parameters, request body, responses and errors."
                },
                "title": {
                    "type": "string",
                    "description": "Short name for the artifact, normally the method being documented (e.g. \"Создание документа\")."
                },
                "purpose": {
                    "type": "string",
                    "description": "One or two sentences, in the user's language, saying what you need it for and which document section it will fill. Shown verbatim on the card and in the builder."
                },
                "prefill": {
                    "type": "object",
                    "description": "Optional. Whatever you already know, so the user edits rather than types from scratch — e.g. the HTTP method and path from an OpenAPI spec, or the standard A-* header block. Same field names as the artifact's own content; partial objects are fine and unknown parts are left empty. Never guess values here: a wrong prefill is worse than an empty form.",
                    "additionalProperties": true
                }
            },
            "required": ["kind", "title", "purpose"]
        }),
    }
}

/// The `artifact` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "artifact".to_string(),
        description:
            "Work with artifacts — structured documents stored for this repository and kept across conversations. `op: \"list\"` returns id, kind, title and a one-line summary of every one; `op: \"read\"` returns one in full together with its rendered output (AsciiDoc tables for an httpRequest, Jira wiki markup for a jiraTicket).\n\n`op: \"create\"` and `op: \"update\"` are how you write a document *you* author — today that means `jiraTicket`, a Jira task description. Create one whenever the user asks you to draft a ticket, task or story; the user gets an editable tab and the ready-to-paste description. Refine it with `update` on the same id instead of creating a second artifact — `content` replaces the stored content wholesale, so send the whole ticket back, including the parts you are not changing. Read it first if you no longer have it.\n\nSend plain text in every field: section headings, numbering, bullets and the link grouping are all produced by the renderer, in this tracker's own wiki-markup format. Do not write Markdown (`##`, `- [ ]`, `|---|`) or Jira markup by hand — it would be rendered as literal text.\n\nAn `httpRequest` cannot be created or updated this way: its content is facts about a real endpoint that only the user has, so use `requestArtifact` to have them fill it in."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "op": {
                    "type": "string",
                    "enum": ["list", "read", "create", "update"],
                    "description": "\"list\" for the summaries, \"read\" for one in full, \"create\" to author a new one, \"update\" to rewrite one you authored."
                },
                "id": {
                    "type": "string",
                    "description": "Artifact id. Required for \"read\" and \"update\"."
                },
                "kind": {
                    "type": "string",
                    "enum": ["jiraTicket"],
                    "description": "Required for \"create\" and \"update\" — says how to read `content`."
                },
                "title": {
                    "type": "string",
                    "description": "Short name, in the user's language. Required for \"create\"; for \"update\" omit it to keep the current one."
                },
                "content": {
                    "type": "object",
                    "description": "The document body, as plain text in every field. For \"jiraTicket\": `why` (why the task exists — the problem, not the solution), `outcome` (target state, phrased «Пользователь может …»), `inScope`/`outOfScope` (string arrays), `solution` (technical approach, if known), `acceptanceCriteria` and `definitionOfDone` (string arrays, one checkable statement per entry), `risks` (string array), `links` (array of {kind, url, title} — `kind` is normally GIT, CONFLUENCE or FIGMA and becomes a sub-heading; `title` is optional). Every field is optional and an empty one is simply not rendered — omit a section rather than filling it with a placeholder. Follow the `jira-task-description` skill for what belongs in each.",
                    "additionalProperties": true
                }
            },
            "required": ["op"]
        }),
    }
}
