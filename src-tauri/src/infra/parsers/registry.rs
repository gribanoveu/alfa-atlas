use crate::domain::supported_files::extension_of;
use crate::domain::workspace_index::{DocumentType, ParsedDocument};

use super::ascii_doc;
use super::json;
use super::markdown;
use super::mermaid;
use super::plantuml;
use super::text;
use super::yaml;

/// Dispatcher that selects a parser by file extension.
#[derive(Debug, Clone, Default)]
pub struct ParserRegistry;

impl ParserRegistry {
    pub fn new() -> Self {
        Self
    }

    /// Determine the `DocumentType` for a path. Returns `None` for unsupported files.
    pub fn doc_type(&self, path: &str) -> Option<DocumentType> {
        match extension_of(path).as_str() {
            ".adoc" | ".asciidoc" => Some(DocumentType::AsciiDoc),
            ".md" | ".markdown" => Some(DocumentType::Markdown),
            ".json" => Some(DocumentType::Json),
            ".yaml" | ".yml" => Some(DocumentType::Yaml),
            ".txt" => Some(DocumentType::Text),
            ".puml" | ".plantuml" => Some(DocumentType::PlantUml),
            ".mmd" | ".mermaid" => Some(DocumentType::Mermaid),
            _ => None,
        }
    }

    /// Parse `content` according to the document type inferred from `path`.
    /// Returns an empty `ParsedDocument` for unknown extensions.
    pub fn parse(&self, path: &str, content: &str) -> ParsedDocument {
        let Some(doc_type) = self.doc_type(path) else {
            return ParsedDocument::default();
        };
        match doc_type {
            DocumentType::AsciiDoc => ascii_doc::parse(content),
            DocumentType::Markdown => markdown::parse(content),
            DocumentType::Json => json::parse(content),
            DocumentType::Yaml => yaml::parse(content),
            DocumentType::Text => text::parse(content),
            DocumentType::PlantUml => plantuml::parse(content),
            DocumentType::Mermaid => mermaid::parse(content),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_type_for_known_extensions() {
        let r = ParserRegistry::new();
        assert_eq!(r.doc_type("a.adoc"), Some(DocumentType::AsciiDoc));
        assert_eq!(r.doc_type("a.md"), Some(DocumentType::Markdown));
        assert_eq!(r.doc_type("a.json"), Some(DocumentType::Json));
        assert_eq!(r.doc_type("a.yml"), Some(DocumentType::Yaml));
        assert_eq!(r.doc_type("a.txt"), Some(DocumentType::Text));
        assert_eq!(r.doc_type("a.puml"), Some(DocumentType::PlantUml));
        assert_eq!(r.doc_type("a.mmd"), Some(DocumentType::Mermaid));
        assert_eq!(r.doc_type("a.rs"), None);
    }

    #[test]
    fn parse_unknown_returns_empty() {
        let r = ParserRegistry::new();
        let parsed = r.parse("foo.rs", "fn main() {}");
        assert!(parsed.anchors.is_empty());
        assert!(parsed.includes.is_empty());
    }
}
