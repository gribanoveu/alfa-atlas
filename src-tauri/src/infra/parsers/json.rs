//! JSON parser — records parse errors as diagnostics, and turns any
//! `"$ref"` string entry into an `Include` (see `super::ref_utils`), so
//! `WorkspaceIndex` can build the same forward/reverse dependency graph for
//! JSON `$ref`s that it already builds for AsciiDoc `include::`/`xref:`.

use crate::domain::workspace_index::{
    Diagnostic, DiagnosticKind, Include, ParsedDocument, Severity,
};

use super::ascii_doc::DOC_PLACEHOLDER;
use super::ref_utils::ref_file_part;

pub fn parse(content: &str) -> ParsedDocument {
    let mut out = ParsedDocument::default();
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(value) => collect_refs(&value, &mut out),
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

/// Recursively walks every object/array looking for a `"$ref": "<string>"`
/// entry. `serde_json::Value` carries no position info, so every resulting
/// `Include` gets a placeholder `line`/`column` of 1 — this only needs to
/// produce file-level dependency edges, not click-to-location diagnostics.
fn collect_refs(value: &serde_json::Value, out: &mut ParsedDocument) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(raw)) = map.get("$ref") {
                if let Some(path) = ref_file_part(raw) {
                    out.includes.push(Include {
                        path: path.to_string(),
                        source_document: DOC_PLACEHOLDER.into(),
                        line: 1,
                        column: 1,
                    });
                }
            }
            for v in map.values() {
                collect_refs(v, out);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                collect_refs(v, out);
            }
        }
        _ => {}
    }
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

    #[test]
    fn relative_ref_becomes_an_include() {
        let parsed = parse(r#"{"foo": {"$ref": "./schemas/common.json#/Bar"}}"#);
        assert_eq!(parsed.includes.len(), 1);
        assert_eq!(parsed.includes[0].path, "./schemas/common.json");
    }

    #[test]
    fn pure_pointer_ref_is_not_an_include() {
        let parsed = parse(r##"{"foo": {"$ref": "#/definitions/Bar"}}"##);
        assert!(parsed.includes.is_empty());
    }

    #[test]
    fn external_url_ref_is_not_an_include() {
        let parsed = parse(r#"{"foo": {"$ref": "https://example.com/schema.json"}}"#);
        assert!(parsed.includes.is_empty());
    }

    #[test]
    fn nested_ref_inside_array_is_found() {
        let parsed = parse(r#"{"items": [{"$ref": "./a.json"}, {"$ref": "./b.json"}]}"#);
        let paths: Vec<_> = parsed.includes.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, vec!["./a.json", "./b.json"]);
    }
}
