//! Minimal PlantUML parser — checks `@startuml` / `@enduml` pairing only.

use crate::domain::workspace_index::{
    Diagnostic, DiagnosticKind, ParsedDocument, Severity,
};

pub fn parse(content: &str) -> ParsedDocument {
    let mut out = ParsedDocument::default();
    let has_start = content.lines().any(|l| l.trim().eq_ignore_ascii_case("@startuml"));
    let has_end = content.lines().any(|l| l.trim().eq_ignore_ascii_case("@enduml"));

    if !has_start {
        out.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::MissingInclude, // reused as "syntax error"
            message: "PlantUML diagram missing @startuml".to_string(),
            document: "".into(),
            line: 1,
            column: 1,
            severity: Severity::Warning,
        });
    } else if !has_end {
        out.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::MissingInclude,
            message: "PlantUML diagram missing @enduml".to_string(),
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
    fn valid_pair_is_clean() {
        let parsed = parse("@startuml\nA -> B\n@enduml\n");
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn missing_start_warns() {
        let parsed = parse("A -> B\n");
        assert_eq!(parsed.diagnostics.len(), 1);
    }
}
