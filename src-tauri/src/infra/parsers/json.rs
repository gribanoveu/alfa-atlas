//! JSON parser — records parse errors as diagnostics, no extracted entities.

use crate::domain::workspace_index::{
    Diagnostic, DiagnosticKind, ParsedDocument, Severity,
};

pub fn parse(content: &str) -> ParsedDocument {
    let mut out = ParsedDocument::default();
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(_) => {}
        Err(e) => {
            let line = e.line().max(1) as u32;
            let column = e.column().max(1) as u32;
            out.diagnostics.push(Diagnostic {
                kind: DiagnosticKind::MissingInclude, // reused as "syntax error"
                message: format!("invalid JSON: {e}"),
                document: "".into(),
                line,
                column,
                severity: Severity::Warning,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_json_is_clean() {
        let parsed = parse(r#"{"a": 1}"#);
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn invalid_json_records_diagnostic() {
        let parsed = parse("{ not json");
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.diagnostics[0].severity, Severity::Warning);
    }
}
