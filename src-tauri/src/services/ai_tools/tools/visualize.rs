//! `visualize` — the assistant draws something for the user.
//!
//! Unusual among the tools in this directory: it touches neither the
//! filesystem nor any store. The rendering happens entirely in the webview
//! (`src/components/Visuals/VisualView.tsx`, which reuses the same Mermaid
//! and PlantUML viewers the AsciiDoc preview already ships), and the source
//! itself lives on the chat's own tool-call block, which `chat_store`
//! already persists. So all this executor does is sanitize what the model
//! sent, mint an id for the chat card and the editor tab to agree on, and
//! hand back a one-line confirmation.
//!
//! Everything the model may ask for must be something the app can actually
//! draw — the schema below lists only implemented kinds and formats, which
//! is why `kind` and `format` are closed enums rather than free strings.

use crate::domain::ai_tools::{ToolError, ToolResult, VisualContent, VisualizeArgs};
use crate::domain::llm::LlmToolDefinition;
use uuid::Uuid;

/// Refuse anything past this. A diagram a person can read is a few hundred
/// lines at most; past this the model has pasted a file, and the chat blob
/// (which carries these args verbatim) pays for it on every later save.
const MAX_SOURCE_BYTES: usize = 100 * 1024;

/// Mermaid diagram types the bundled renderer understands — quoted in the
/// tool description so the model picks from what will actually draw.
const MERMAID_DIAGRAM_TYPES: &str = "flowchart, sequenceDiagram, classDiagram, stateDiagram-v2, erDiagram, journey, gantt, mindmap, timeline, gitGraph, C4Context";

/// Strips a markdown code fence the model wrapped its source in, then any
/// leading/trailing blank lines. Models emit ```` ```mermaid ```` around
/// diagram source often enough that rejecting it (or passing it through to
/// a renderer that then fails on line 1) is just a worse product than
/// quietly accepting it.
fn unfence(raw: &str) -> String {
    let trimmed = raw.trim();
    let inner = match trimmed.strip_prefix("```") {
        // Only unwrap a fence that is actually closed — an unterminated
        // ``` is more likely part of the diagram than a wrapper.
        Some(rest) => match rest.strip_suffix("```") {
            // Drop the info string (`mermaid`, `plantuml`, …) on the
            // opening fence line.
            Some(body) => body.split_once('\n').map(|(_, b)| b).unwrap_or(body),
            None => trimmed,
        },
        None => trimmed,
    };
    let mut lines: Vec<&str> = inner.split('\n').collect();
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

fn invalid(reason: &str) -> ToolError {
    ToolError::InvalidArguments {
        tool: "visualize".to_string(),
        reason: reason.to_string(),
    }
}

/// One-line confirmation for the model — enough to know the call landed and
/// what it produced, without echoing the source back at it.
fn summarize(content: &VisualContent) -> String {
    match content {
        VisualContent::Diagram { format, source } => {
            let lines = source.lines().count();
            format!("{} diagram, {} lines, rendered in a tab", format.label(), lines)
        }
    }
}

pub(super) fn visualize(args: VisualizeArgs) -> Result<ToolResult, ToolError> {
    let title = args.title.trim().to_string();
    if title.is_empty() {
        return Err(invalid("title must not be empty"));
    }

    let content = match args.content {
        VisualContent::Diagram { format, source } => {
            if source.len() > MAX_SOURCE_BYTES {
                return Err(invalid(&format!(
                    "diagram source is {} bytes, over the {} byte limit — draw a smaller diagram, or split it into several",
                    source.len(),
                    MAX_SOURCE_BYTES
                )));
            }
            let source = unfence(&source);
            if source.is_empty() {
                return Err(invalid("diagram source must not be empty"));
            }
            VisualContent::Diagram { format, source }
        }
    };

    Ok(ToolResult::VisualShown {
        visual_id: Uuid::new_v4().to_string(),
        kind: content.kind_name().to_string(),
        title,
        summary: summarize(&content),
    })
}

/// The `visualize` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "visualize".to_string(),
        description:
            "Draw a diagram for the user and put it in the chat as a card they can open in a tab. Use it whenever an explanation is really about structure or flow — «как это работает», «как устроен …», the path a request takes through the code, the states an entity moves between, how modules depend on each other — and prefer it over drawing boxes and arrows with characters in your prose, which is unreadable by comparison. Best paired with a short answer: call `visualize` once, then explain in a few sentences; the card already offers the «Просмотр» button, so do not repeat the diagram source in your reply. Ground the diagram in code you actually read, name real modules/functions, and keep it to what fits on one screen — several focused diagrams beat one exhaustive one. The diagram is shown to the user, it is not saved into the repository (the user can save it themselves from the tab); to put one *into* a document, write a `[mermaid]`/`[plantuml]` block with `writeFile`/`editFile` instead. Only the kinds and formats listed here can be rendered — do not invent others."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["diagram"],
                    "description": "Which sort of visualization. \"diagram\" renders diagram source into a zoomable picture."
                },
                "title": {
                    "type": "string",
                    "description": "Short name, in the user's language — it labels the chat card and the editor tab (e.g. «Цикл вызова инструментов»)."
                },
                "caption": {
                    "type": "string",
                    "description": "Optional one-sentence note shown under the title, in the user's language: what the diagram shows or where to start reading it."
                },
                "format": {
                    "type": "string",
                    "enum": ["mermaid", "plantuml"],
                    "description": "kind \"diagram\" only. Prefer \"mermaid\" — it renders far faster. Use \"plantuml\" when the diagram needs something Mermaid cannot express, or to match diagrams already in this repository."
                },
                "source": {
                    "type": "string",
                    "description": format!(
                        "kind \"diagram\" only. The diagram source itself, with no surrounding markdown code fence and no AsciiDoc block delimiters. Mermaid supports: {MERMAID_DIAGRAM_TYPES} — pick one of those and nothing else. Label nodes in the user's language, keep identifiers ASCII, and quote any label containing brackets or punctuation."
                    )
                }
            },
            "required": ["kind", "title", "format", "source"]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai_tools::DiagramFormat;

    fn diagram(source: &str) -> VisualizeArgs {
        VisualizeArgs {
            title: "Поток данных".to_string(),
            caption: None,
            content: VisualContent::Diagram {
                format: DiagramFormat::Mermaid,
                source: source.to_string(),
            },
        }
    }

    #[test]
    fn a_plain_diagram_gets_an_id_and_a_summary() {
        let result = visualize(diagram("flowchart TD\n  a-->b")).expect("visualize");
        match result {
            ToolResult::VisualShown { visual_id, kind, title, summary } => {
                assert!(!visual_id.is_empty());
                assert_eq!(kind, "diagram");
                assert_eq!(title, "Поток данных");
                assert_eq!(summary, "mermaid diagram, 2 lines, rendered in a tab");
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn a_markdown_fence_around_the_source_is_stripped() {
        // Models wrap diagram source in ```mermaid constantly; passing it
        // through would fail at the renderer's first line.
        assert_eq!(unfence("```mermaid\nflowchart TD\n  a-->b\n```"), "flowchart TD\n  a-->b");
        assert_eq!(unfence("```\nflowchart TD\n```"), "flowchart TD");
    }

    #[test]
    fn an_unterminated_fence_is_left_alone() {
        // More likely part of the diagram than a wrapper the model forgot
        // to close, and mangling it silently would be worse than a render
        // error the user can see.
        assert_eq!(unfence("```mermaid\nflowchart TD"), "```mermaid\nflowchart TD");
    }

    #[test]
    fn blank_lines_around_the_source_are_trimmed() {
        assert_eq!(unfence("\n\nflowchart TD\n  a-->b\n\n"), "flowchart TD\n  a-->b");
    }

    #[test]
    fn an_empty_source_is_rejected() {
        assert!(visualize(diagram("   \n  ")).is_err());
        // A fence with nothing in it is empty too, not a one-line diagram.
        assert!(visualize(diagram("```mermaid\n```")).is_err());
    }

    #[test]
    fn an_empty_title_is_rejected() {
        let mut args = diagram("flowchart TD\n  a-->b");
        args.title = "  ".to_string();
        assert!(visualize(args).is_err());
    }

    #[test]
    fn an_oversized_source_is_rejected() {
        let huge = "x".repeat(MAX_SOURCE_BYTES + 1);
        assert!(visualize(diagram(&huge)).is_err());
    }

    #[test]
    fn the_schema_offers_only_renderable_kinds_and_formats() {
        // The whole point of closed enums here: the model must not be able
        // to name a kind or format nothing in the app can draw.
        let def = definition();
        assert_eq!(def.parameters["properties"]["kind"]["enum"], serde_json::json!(["diagram"]));
        assert_eq!(
            def.parameters["properties"]["format"]["enum"],
            serde_json::json!(["mermaid", "plantuml"])
        );
    }
}
