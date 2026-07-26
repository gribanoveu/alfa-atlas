//! YAML parser — records parse errors as diagnostics.

use crate::domain::workspace_index::{
    Diagnostic, DiagnosticKind, ParsedDocument, Severity,
};

pub fn parse(content: &str) -> ParsedDocument {
    let mut out = ParsedDocument::default();
    if let Err(e) = serde_yaml::from_str::<serde_yaml::Value>(content) {
        let (line, column) = error_position(&e);
        out.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::MissingInclude, // reused as "syntax error"
            message: format!("invalid YAML: {e}"),
            document: "".into(),
            line,
            column,
            severity: Severity::Warning,
        });
    }
    out
}

fn error_position(e: &serde_yaml::Error) -> (u32, u32) {
    if let Some(loc) = e.location() {
        (loc.line().max(1) as u32, loc.column().max(1) as u32)
    } else {
        (1, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_yaml_is_clean() {
        let parsed = parse("key: value\n");
        assert!(parsed.diagnostics.is_empty());
    }
}
