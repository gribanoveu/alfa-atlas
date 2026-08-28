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

use crate::domain::ai_tools::{ArtifactReadArgs, ToolError, ToolResult};
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
            "Read artifacts the user has already filled in for this repository. `op: \"list\"` returns id, kind, title and a one-line summary of every finished artifact; `op: \"read\"` returns one artifact in full, together with its ready-made AsciiDoc (parameter tables, curl example, response examples, error table). Artifacts persist across conversations, so use this when the user refers to a request they described earlier, or when the artifact list in your context mentions one relevant to the document you are writing. To have a *new* one filled in, use `requestArtifact` instead."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "op": {
                    "type": "string",
                    "enum": ["list", "read"],
                    "description": "\"list\" for the summaries, \"read\" for one artifact in full."
                },
                "id": {
                    "type": "string",
                    "description": "Artifact id. Required for op \"read\", ignored for \"list\"."
                }
            },
            "required": ["op"]
        }),
    }
}
