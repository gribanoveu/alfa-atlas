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

use crate::domain::ai_tools::{DiagramFormat, ToolError, ToolResult, VisualContent, VisualizeArgs};
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

/// Block openers that must be closed with a bare `end`. Covers both
/// sequence diagrams (`alt`/`opt`/`loop`/`par`/`critical`/`break`/`rect`/
/// `box`) and flowcharts (`subgraph`) — an unbalanced one is a guaranteed
/// parse failure, which today is discovered only when someone opens the
/// card.
const MERMAID_BLOCK_OPENERS: &[&str] = &[
    "alt", "opt", "loop", "par", "critical", "break", "rect", "box", "subgraph",
];

/// Diagram headers the bundled renderer accepts. A superset of
/// `MERMAID_DIAGRAM_TYPES` (which is advice aimed at the model): the
/// aliases below render perfectly well, and this list only exists to catch
/// source that is not a diagram at all.
const MERMAID_HEADERS: &[&str] = &[
    "flowchart",
    "graph",
    "sequenceDiagram",
    "classDiagram",
    "stateDiagram-v2",
    "stateDiagram",
    "erDiagram",
    "journey",
    "gantt",
    "mindmap",
    "timeline",
    "gitGraph",
    "C4Context",
    "pie",
];

/// The first line that actually declares the diagram — skipping blank
/// lines, `%%` comments/directives, and a leading `---` front-matter block.
fn mermaid_header_line(source: &str) -> Option<&str> {
    let mut lines = source.lines().map(str::trim).peekable();
    if lines.peek() == Some(&"---") {
        lines.next();
        for line in lines.by_ref() {
            if line == "---" {
                break;
            }
        }
    }
    lines.find(|line| !line.is_empty() && !line.starts_with("%%"))
}

/// True when `line` opens a block: the keyword must be the whole line or be
/// followed by whitespace, so a flowchart node named `alt[…]` or a message
/// like `A->>B: alt path` is not mistaken for one.
fn opens_a_block(line: &str) -> bool {
    MERMAID_BLOCK_OPENERS.iter().any(|kw| {
        line.strip_prefix(kw)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    })
}

/// In a sequence diagram `;` is a statement separator, so a message, note
/// or alias containing one is cut in half and the remainder parsed as its
/// own statement — a hard parse error. Verified against the bundled
/// mermaid: `A->>B: один; два`, the same inside `note over`, inside a
/// participant alias, and even inside parentheses all fail; a *trailing*
/// `;` is fine (it terminates the statement), and in a flowchart label a
/// `;` is harmless. Hence "a `;` with something after it, in a sequence
/// diagram" rather than a blanket ban.
///
/// This is the one construct that actually broke a real diagram in the
/// field: the model wrote «пакет SIGNED; нет пакета — пропуск», the card
/// failed to render, and it cost a whole extra round to discover.
fn semicolon_splits_a_statement(line: &str) -> bool {
    line.split_once(';')
        .is_some_and(|(_, rest)| !rest.trim().is_empty())
}

/// Cheap syntax gate for Mermaid, run before the call is reported as a
/// success. Only checks things that are *certainly* broken — verified
/// against the bundled renderer, not guessed — because a false rule here
/// blocks a diagram that would have drawn fine. A real render (the chat
/// card) is what catches everything else. The point is that the model finds
/// out inside its own turn, with no user round trip: an `InvalidArguments`
/// here comes straight back as a tool error it can fix, *before* it has
/// told the user the diagram is ready.
fn lint_mermaid(source: &str) -> Result<(), ToolError> {
    let Some(header) = mermaid_header_line(source) else {
        return Err(invalid("diagram source has no diagram declaration"));
    };
    let declares_a_diagram = MERMAID_HEADERS.iter().any(|kind| {
        header
            .strip_prefix(kind)
            .is_some_and(|rest| rest.is_empty() || !rest.starts_with(|c: char| c.is_alphanumeric()))
    });
    if !declares_a_diagram {
        return Err(invalid(&format!(
            "diagram source must start with one of: {MERMAID_DIAGRAM_TYPES} — got \"{header}\""
        )));
    }

    let is_sequence = header.starts_with("sequenceDiagram");

    let mut depth: i32 = 0;
    for line in source.lines().map(str::trim) {
        if is_sequence && !line.starts_with("%%") && semicolon_splits_a_statement(line) {
            return Err(invalid(&format!(
                "a `;` inside a sequence diagram splits the line into two statements and fails to parse — replace it with a comma or a dash: \"{line}\""
            )));
        }
        if opens_a_block(line) {
            depth += 1;
        } else if line == "end" {
            depth -= 1;
            if depth < 0 {
                return Err(invalid(
                    "diagram source has an `end` with no matching alt/opt/loop/par/rect/subgraph block",
                ));
            }
        }
    }
    if depth > 0 {
        return Err(invalid(&format!(
            "diagram source leaves {depth} alt/opt/loop/par/rect/subgraph block(s) unclosed — every one needs its own `end`"
        )));
    }
    Ok(())
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
            if format == DiagramFormat::Mermaid {
                lint_mermaid(&source)?;
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
                        "kind \"diagram\" only. The diagram source itself, with no surrounding markdown code fence and no AsciiDoc block delimiters. Mermaid supports: {MERMAID_DIAGRAM_TYPES} — pick one of those and nothing else. Label nodes in the user's language, keep identifiers ASCII, and quote any label containing brackets or punctuation. Never put a `;` in a sequence diagram unless it ends the line — it separates statements, so «SIGNED; нет пакета» fails to parse; use a comma or a dash. Never set colours yourself (`rect rgb(...)`, `style`, `classDef` with literal hex): the app themes the diagram to the user's light/dark setting, and a hardcoded light fill is unreadable in the dark one. Close every `alt`/`opt`/`loop`/`par`/`rect`/`subgraph` with its own `end`."
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
    fn source_that_declares_no_diagram_is_rejected() {
        // Prose, or a stray explanation the model pasted instead of a
        // diagram — today this reports success and fails at the renderer.
        assert!(visualize(diagram("Поток такой: контроллер вызывает сервис")).is_err());
        assert!(visualize(diagram("%% только комментарий")).is_err());
    }

    #[test]
    fn a_front_matter_block_and_comments_do_not_hide_the_header() {
        assert!(visualize(diagram("---\ntitle: Поток\n---\nsequenceDiagram\n  A->>B: x")).is_ok());
        assert!(visualize(diagram("%%{init: {'theme':'dark'}}%%\nflowchart TD\n  a-->b")).is_ok());
    }

    #[test]
    fn unbalanced_blocks_are_rejected() {
        let unclosed = "sequenceDiagram\n  alt найден\n    A->>B: ok";
        assert!(visualize(diagram(unclosed)).is_err());

        let stray_end = "sequenceDiagram\n  A->>B: ok\n  end";
        assert!(visualize(diagram(stray_end)).is_err());

        let balanced = "sequenceDiagram\n  alt найден\n    A->>B: ok\n  else нет\n    A->>B: no\n  end";
        assert!(visualize(diagram(balanced)).is_ok());
    }

    #[test]
    fn a_semicolon_that_splits_a_sequence_statement_is_rejected() {
        // The exact line that broke a real diagram in the field.
        let real = "sequenceDiagram\n    CB->>DB: пакет SIGNED / UNSIGNED; нет пакета — тихий пропуск";
        assert!(visualize(diagram(real)).is_err());
        // Same in a note, and in a participant alias.
        assert!(visualize(diagram("sequenceDiagram\n    note over A,B: один; два")).is_err());
        assert!(
            visualize(diagram("sequenceDiagram\n    participant A as Один; Два\n    A->>B: x")).is_err()
        );
        // The error has to name the fix, not just the fault.
        let err = visualize(diagram(real)).unwrap_err().to_string();
        assert!(err.contains("comma or a dash"), "got {err}");
    }

    #[test]
    fn a_semicolon_the_renderer_tolerates_is_not_rejected() {
        // Verified against the bundled mermaid: a trailing `;` terminates
        // the statement, and in a flowchart label it is ordinary text.
        // Rejecting either would block diagrams that draw fine.
        assert!(visualize(diagram("sequenceDiagram\n    A->>B: x;")).is_ok());
        assert!(visualize(diagram("flowchart TD\n    a[один; два]-->b")).is_ok());
    }

    #[test]
    fn punctuation_the_renderer_accepts_is_left_alone() {
        // These were suspected of breaking the renderer and turned out not
        // to — parentheses, a slash and `<br>` in a participant alias all
        // parse. A lint rule for them would cost real diagrams for nothing.
        let src = "sequenceDiagram\n    participant NDG as Gateway (annual-tax-report-api)\n    participant SM as Модуль / alfacapture<br>(предположение)\n    NDG->>SM: статус → READY_FOR_SEND";
        assert!(visualize(diagram(src)).is_ok());
    }

    #[test]
    fn a_block_keyword_inside_a_label_is_not_a_block() {
        // `alt[...]` is a node id, `: alt path` is message text — neither
        // opens a block, and treating them as one would reject valid
        // diagrams.
        let flow = "flowchart TD\n  alt[Альтернатива]-->b\n  b-->c";
        assert!(visualize(diagram(flow)).is_ok());
        let seq = "sequenceDiagram\n  A->>B: alt path taken";
        assert!(visualize(diagram(seq)).is_ok());
    }

    #[test]
    fn plantuml_source_is_not_mermaid_linted() {
        // The Mermaid header list says nothing about `@startuml`; only the
        // real renderer can judge PlantUML.
        let args = VisualizeArgs {
            title: "Поток".to_string(),
            caption: None,
            content: VisualContent::Diagram {
                format: DiagramFormat::Plantuml,
                source: "@startuml\nA -> B\n@enduml".to_string(),
            },
        };
        assert!(visualize(args).is_ok());
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
