//! Minimal Mermaid parser — checks for a diagram-type declaration.

use crate::domain::workspace_index::{
    Diagnostic, DiagnosticKind, ParsedDocument, Severity,
};

pub fn parse(content: &str) -> ParsedDocument {
    let mut out = ParsedDocument::default();
    let first_non_blank = content
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_ascii_lowercase())
        .unwrap_or_default();

    let starts_with_diagram_type =
        first_non_blank.starts_with("flowchart") || first_non_blank.starts_with("graph");

    if !starts_with_diagram_type && !first_non_blank.is_empty() {
        out.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::MissingInclude, // reused as "syntax error"
            message: "Mermaid diagram should start with `flowchart` or `graph`".to_string(),
            document: "".into(),
            line: 1,
            column: 1,
            severity: Severity::Warning,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flowchart_is_clean() {
        let parsed = parse("flowchart TD\n  A --> B\n");
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn unknown_header_warns() {
        let parsed = parse("something else\n");
        assert_eq!(parsed.diagnostics.len(), 1);
    }
}
