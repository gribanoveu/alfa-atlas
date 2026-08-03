//! Tree-sitter-backed section-title extraction for AsciiDoc, via
//! `tree-sitter-asciidoc`.
//!
//! Supersedes a hand-written line scan (`=`/`==`/... counted at line start)
//! specifically because that approach has no way to tell a real section
//! title from an `=` that merely starts a line inside a listing/literal
//! block or table — a grammar-aware parser doesn't have that failure mode.
//! This is the same "don't guess, use a real parser when one exists"
//! principle applied to Java; `JsonIndexer`/`YamlIndexer` remain
//! symbol-less specifically because no comparable position-aware
//! JSON/YAML crate was pulled in for this pass (see those modules' docs).
//!
//! AsciiDoc was prioritized over Kotlin when `tree-sitter-asciidoc`'s
//! compiled grammar turned out to need a newer tree-sitter ABI (15) than
//! `tree-sitter-kotlin` (still capped at `tree-sitter <0.23`) supports —
//! `tree-sitter`'s `links` constraint only allows one version project-wide,
//! so this project now tracks current `tree-sitter`/`tree-sitter-java`, and
//! `KotlinIndexer` no longer uses tree-sitter (see that module's docs).
//!
//! `infra/parsers/ascii_doc.rs` remains the production parser for the
//! editor's cross-reference index (anchors/includes/xrefs/attributes/
//! images) — this indexer is separate and only extracts section titles.

use tree_sitter::{Node, Parser};

use crate::domain::repo_index::{LanguageFacts, LanguageIndexer, Symbol, SymbolKind};

pub struct AsciiDocIndexer;

/// `document_title` is the `=` document title; `title1`..`title5` are
/// `==` through `======` section titles (confirmed against the grammar's
/// `node-types.json`).
const TITLE_NODE_KINDS: &[&str] = &[
    "document_title",
    "title1",
    "title2",
    "title3",
    "title4",
    "title5",
];

impl LanguageIndexer for AsciiDocIndexer {
    fn index(&self, content: &str) -> LanguageFacts {
        let mut parser = Parser::new();
        if parser.set_language(&tree_sitter_asciidoc::language()).is_err() {
            return LanguageFacts::default();
        }
        let Some(tree) = parser.parse(content, None) else {
            return LanguageFacts::default();
        };

        let mut symbols = Vec::new();
        walk(tree.root_node(), content.as_bytes(), &mut symbols);
        LanguageFacts { symbols }
    }
}

fn walk(node: Node, source: &[u8], out: &mut Vec<Symbol>) {
    if TITLE_NODE_KINDS.contains(&node.kind()) {
        push_title(node, source, out);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, source, out);
    }
}

/// Every title node kind has a `line` child carrying the title text (no
/// named field for it in this grammar — found by node kind, same as the
/// Kotlin indexer has to for identifiers).
fn push_title(node: Node, source: &[u8], out: &mut Vec<Symbol>) {
    let mut cursor = node.walk();
    let Some(line) = node.children(&mut cursor).find(|c| c.kind() == "line") else {
        return;
    };
    let Ok(text) = line.utf8_text(source) else {
        return;
    };
    let name = text.trim().to_string();
    if name.is_empty() {
        return;
    }
    out.push(Symbol {
        name,
        kind: SymbolKind::Section,
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        start_byte: node.start_byte() as u32,
        end_byte: node.end_byte() as u32,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_section_titles_at_multiple_levels() {
        let facts = AsciiDocIndexer.index("= Document Title\n\nintro text\n\n== Errors\n\nmore\n");
        assert_eq!(facts.symbols.len(), 2);
        assert_eq!(facts.symbols[0].name, "Document Title");
        assert_eq!(facts.symbols[0].start_line, 1);
        assert_eq!(facts.symbols[1].name, "Errors");
        assert_eq!(facts.symbols[1].kind, SymbolKind::Section);
    }

    #[test]
    fn ignores_equals_signs_inside_listing_blocks() {
        // The exact failure mode a hand-written line scan can't avoid but a
        // real grammar does: `=` at line start inside a literal block is
        // not a section title.
        let facts = AsciiDocIndexer.index("----\n= not a title\n----\n");
        assert!(facts.symbols.is_empty());
    }

    #[test]
    fn does_not_panic_on_malformed_input() {
        let facts = AsciiDocIndexer.index("== unterminated [source\n");
        let _ = facts;
    }
}
