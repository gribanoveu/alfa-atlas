//! Plain-text parser — no syntax analysis, no extracted entities.

use crate::domain::workspace_index::ParsedDocument;

pub fn parse(_content: &str) -> ParsedDocument {
    ParsedDocument::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_returns_empty() {
        let parsed = parse("anything goes here\n");
        assert!(parsed.anchors.is_empty());
        assert!(parsed.diagnostics.is_empty());
    }
}
