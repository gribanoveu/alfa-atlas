//! YAML parser — records parse errors as diagnostics, and turns any
//! `"$ref"` string entry into an `Include` (see `super::ref_utils`), so
//! `WorkspaceIndex` can build the same forward/reverse dependency graph for
//! YAML `$ref`s that it already builds for AsciiDoc `include::`/`xref:`.

use crate::domain::workspace_index::{
    Diagnostic, DiagnosticKind, Include, ParsedDocument, Severity,
};

use super::ascii_doc::DOC_PLACEHOLDER;
use super::ref_utils::ref_file_part;

pub fn parse(content: &str) -> ParsedDocument {
    let mut out = ParsedDocument::default();
    match serde_yaml::from_str::<serde_yaml::Value>(content) {
        Ok(value) => collect_refs(&value, &mut out),
        Err(e) => {
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

/// Recursively walks every mapping/sequence looking for a `"$ref": "<string>"`
/// entry. `serde_yaml::Value` carries no position info, so every resulting
/// `Include` gets a placeholder `line`/`column` of 1 — this only needs to
/// produce file-level dependency edges, not click-to-location diagnostics.
fn collect_refs(value: &serde_yaml::Value, out: &mut ParsedDocument) {
    match value {
        serde_yaml::Value::Mapping(map) => {
            for (k, v) in map.iter() {
                if let (serde_yaml::Value::String(key), serde_yaml::Value::String(raw)) = (k, v) {
                    if key == "$ref" {
                        if let Some(path) = ref_file_part(raw) {
                            out.includes.push(Include {
                                path: path.to_string(),
                                source_document: DOC_PLACEHOLDER.into(),
                                line: 1,
                                column: 1,
                            });
                        }
                    }
                }
            }
            for v in map.values() {
                collect_refs(v, out);
            }
        }
        serde_yaml::Value::Sequence(items) => {
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
    fn valid_yaml_is_clean() {
        let parsed = parse("key: value\n");
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn relative_ref_becomes_an_include() {
        let parsed = parse("foo:\n  $ref: ./schemas/common.yaml#/Bar\n");
        assert_eq!(parsed.includes.len(), 1);
        assert_eq!(parsed.includes[0].path, "./schemas/common.yaml");
    }

    #[test]
    fn pure_pointer_ref_is_not_an_include() {
        let parsed = parse("foo:\n  $ref: \"#/definitions/Bar\"\n");
        assert!(parsed.includes.is_empty());
    }

    #[test]
    fn external_url_ref_is_not_an_include() {
        let parsed = parse("foo:\n  $ref: https://example.com/schema.yaml\n");
        assert!(parsed.includes.is_empty());
    }

    #[test]
    fn nested_ref_inside_sequence_is_found() {
        let parsed = parse("items:\n  - $ref: ./a.yaml\n  - $ref: ./b.yaml\n");
        let paths: Vec<_> = parsed.includes.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, vec!["./a.yaml", "./b.yaml"]);
    }
}
