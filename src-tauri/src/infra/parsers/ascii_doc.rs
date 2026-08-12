//! Lightweight line/regex-based AsciiDoc parser.
//!
//! Extracts the subset of constructs needed for indexing and diagnostics:
//! `[[id]]` / `[#id]` anchors, `include::path[]`, `xref:doc[#anchor][]`,
//! `:name: value` document attributes, and `image::path[]`. This is intentionally
//! not a full AsciiDoc AST — see `doc/workspace-index/feature-info-part-1.md`.

use crate::domain::workspace_index::{Anchor, Attribute, Image, Include, ParsedDocument, Reference};

/// Document-id sentinel carried by parsers when the source `DocumentId` is
/// not yet known. The caller (`WorkspaceIndex`) rewrites these after parsing.
pub const DOC_PLACEHOLDER: &str = "";

/// Parse AsciiDoc `content` and return the extracted entities.
///
/// `source_document` for all returned entries is set to `DocumentId("")` — the
/// caller rewrites it to the actual document id once known. This keeps the
/// parser pure (no path knowledge required).
pub fn parse(content: &str) -> ParsedDocument {
    let mut out = ParsedDocument::default();

    for (line_idx, line) in content.lines().enumerate() {
        let line_no = (line_idx as u32) + 1;
        scan_line(line, line_no, &mut out);
    }

    out
}

fn scan_line(line: &str, line_no: u32, out: &mut ParsedDocument) {
    let trimmed = line.trim_start();

    // Block anchor: `[[id]]` (also accepts `[[id,Label]]`).
    if let Some(rest) = strip_prefix(trimmed, "[[") {
        if let Some(end) = rest.find("]]") {
            let inner = &rest[..end];
            let id = inner.split(',').next().unwrap_or("").trim();
            if !id.is_empty() {
                let column = (line.find("[[").unwrap_or(0) as u32) + 1;
                out.anchors.push(Anchor {
                    id: id.to_string(),
                    document: DOC_PLACEHOLDER.into(),
                    line: line_no,
                    column,
                });
            }
        }
    }

    // Inline/block anchor: `[#id]` at line start (not `[[...]]` which is handled above).
    if let Some(rest) = strip_prefix(trimmed, "[#") {
        if let Some(end) = rest.find(']') {
            let id = rest[..end].trim();
            if !id.is_empty() && !id.contains('[') {
                let column = (line.find("[#").unwrap_or(0) as u32) + 1;
                out.anchors.push(Anchor {
                    id: id.to_string(),
                    document: DOC_PLACEHOLDER.into(),
                    line: line_no,
                    column,
                });
            }
        }
    }

    // include::path[]
    if let Some(pos) = trimmed.find("include::") {
        if let Some(inner_start) = pos.checked_add("include::".len()) {
            if let Some(rest) = line.get(inner_start..) {
                if let Some(end) = rest.find('[') {
                    let raw = rest[..end].trim();
                    if !raw.is_empty() {
                        let column = (inner_start as u32) + 1;
                        out.includes.push(Include {
                            path: raw.to_string(),
                            source_document: DOC_PLACEHOLDER.into(),
                            line: line_no,
                            column,
                        });
                    }
                }
            }
        }
    }

    // xref:doc[#anchor][]
    if let Some(pos) = trimmed.find("xref:") {
        if let Some(inner_start) = pos.checked_add("xref:".len()) {
            if let Some(rest) = line.get(inner_start..) {
                if let Some(end) = rest.find('[') {
                    let target = rest[..end].trim();
                    if !target.is_empty() {
                        let column = (inner_start as u32) + 1;
                        let (doc, anchor) = split_xref_target(target);
                        out.references.push(Reference {
                            target_document: doc,
                            anchor,
                            source_document: DOC_PLACEHOLDER.into(),
                            line: line_no,
                            column,
                        });
                    }
                }
            }
        }
    }

    // image::path[]
    if let Some(pos) = trimmed.find("image::") {
        if let Some(inner_start) = pos.checked_add("image::".len()) {
            if let Some(rest) = line.get(inner_start..) {
                if let Some(end) = rest.find('[') {
                    let raw = rest[..end].trim();
                    if !raw.is_empty() {
                        out.images.push(Image {
                            path: raw.to_string(),
                            document: DOC_PLACEHOLDER.into(),
                            line: line_no,
                        });
                    }
                }
            }
        }
    }

    // Document attribute: `:name: value` (must start with `:` at line start).
    if let Some(rest) = strip_prefix(trimmed, ":") {
        if !rest.starts_with(':') && !rest.starts_with('[') {
            if let Some(colon) = rest.find(':') {
                let name = rest[..colon].trim();
                if !name.is_empty()
                    && !name.contains(' ')
                    && name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                {
                    let value = rest[colon + 1..].trim();
                    let column = (line.find(':').unwrap_or(0) as u32) + 1;
                    out.attributes.push(Attribute {
                        name: name.to_string(),
                        value: value.to_string(),
                        document: DOC_PLACEHOLDER.into(),
                        line: line_no,
                    });
                    let _ = column; // column unused for attributes per spec
                }
            }
        }
    }
}

fn split_xref_target(target: &str) -> (String, Option<String>) {
    if let Some(hash) = target.find('#') {
        let doc = target[..hash].trim().to_string();
        let anchor = target[hash + 1..].trim().to_string();
        let anchor = if anchor.is_empty() {
            None
        } else {
            Some(anchor)
        };
        (doc, anchor)
    } else {
        (target.to_string(), None)
    }
}

fn strip_prefix<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    s.strip_prefix(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_block_anchor() {
        let parsed = parse("[[installation]]\n= Installation\n");
        assert_eq!(parsed.anchors.len(), 1);
        assert_eq!(parsed.anchors[0].id, "installation");
        assert_eq!(parsed.anchors[0].line, 1);
        assert_eq!(parsed.anchors[0].column, 1);
    }

    #[test]
    fn parses_inline_anchor() {
        let parsed = parse("[#configuration]\n");
        assert_eq!(parsed.anchors.len(), 1);
        assert_eq!(parsed.anchors[0].id, "configuration");
    }

    #[test]
    fn parses_include() {
        let parsed = parse("include::common.adoc[]\n");
        assert_eq!(parsed.includes.len(), 1);
        assert_eq!(parsed.includes[0].path, "common.adoc");
        assert_eq!(parsed.includes[0].line, 1);
    }

    #[test]
    fn parses_xref_with_anchor() {
        let parsed = parse("xref:install.adoc#configuration[]\n");
        assert_eq!(parsed.references.len(), 1);
        assert_eq!(parsed.references[0].target_document, "install.adoc");
        assert_eq!(
            parsed.references[0].anchor.as_deref(),
            Some("configuration")
        );
    }

    #[test]
    fn parses_xref_without_anchor() {
        let parsed = parse("xref:install.adoc[]\n");
        assert_eq!(parsed.references[0].target_document, "install.adoc");
        assert!(parsed.references[0].anchor.is_none());
    }

    #[test]
    fn parses_attribute() {
        let parsed = parse(":product-name: AlfaAtlas\n");
        assert_eq!(parsed.attributes.len(), 1);
        assert_eq!(parsed.attributes[0].name, "product-name");
        assert_eq!(parsed.attributes[0].value, "AlfaAtlas");
    }

    #[test]
    fn parses_image() {
        let parsed = parse("image::images/auth.png[]\n");
        assert_eq!(parsed.images.len(), 1);
        assert_eq!(parsed.images[0].path, "images/auth.png");
    }

    #[test]
    fn ignores_attribute_like_block() {
        // `[[id]]` should not be misread as attribute `:`
        let parsed = parse("[[installation]]\n");
        assert!(parsed.attributes.is_empty());
    }

    #[test]
    fn handles_multiple_constructs_per_doc() {
        let src = "[[intro]]\n:lang: ru\ninclude::a.adoc[]\nxref:b.adoc#sec[]\nimage::x.png[]\n";
        let parsed = parse(src);
        assert_eq!(parsed.anchors.len(), 1);
        assert_eq!(parsed.attributes.len(), 1);
        assert_eq!(parsed.includes.len(), 1);
        assert_eq!(parsed.references.len(), 1);
        assert_eq!(parsed.images.len(), 1);
    }
}
