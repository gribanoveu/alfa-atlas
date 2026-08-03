//! Tree-sitter-backed symbol extraction for Java.
//!
//! This is the swap point mentioned in the Repository Index design:
//! replacing tree-sitter with a JDT Language Server-backed indexer later
//! means writing `JdtLsJavaIndexer: LanguageIndexer` and changing one line
//! in `infra::language_indexers::default_indexers` — nothing else in the
//! index depends on tree-sitter specifically.

use tree_sitter::{Node, Parser};

use crate::domain::repo_index::{LanguageFacts, LanguageIndexer, Symbol, SymbolKind};

pub struct JavaIndexer;

impl LanguageIndexer for JavaIndexer {
    fn index(&self, content: &str) -> LanguageFacts {
        let mut parser = Parser::new();
        if parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .is_err()
        {
            return LanguageFacts::default();
        }
        // Malformed input never fails `parse` — tree-sitter always returns a
        // tree, using ERROR nodes for anything it can't make sense of. Only
        // a parser/grammar setup failure (handled above) yields no tree.
        let Some(tree) = parser.parse(content, None) else {
            return LanguageFacts::default();
        };

        let mut symbols = Vec::new();
        walk(tree.root_node(), content.as_bytes(), &mut symbols);
        LanguageFacts { symbols }
    }
}

fn walk(node: Node, source: &[u8], out: &mut Vec<Symbol>) {
    match node.kind() {
        "class_declaration" => push_named(node, source, SymbolKind::Class, out),
        "interface_declaration" => push_named(node, source, SymbolKind::Interface, out),
        "enum_declaration" => push_named(node, source, SymbolKind::Enum, out),
        "method_declaration" | "constructor_declaration" => {
            push_named(node, source, SymbolKind::Method, out)
        }
        "field_declaration" => push_field_declarators(node, source, out),
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, source, out);
    }
}

/// Declarations with a `name` field (class/interface/enum/method/constructor).
/// The symbol's range is the whole declaration node, not just the name
/// token — useful for a future semantic chunker to know a method's full
/// extent, not just where its name sits.
fn push_named(node: Node, source: &[u8], kind: SymbolKind, out: &mut Vec<Symbol>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let Ok(name) = name_node.utf8_text(source) else {
        return;
    };
    out.push(symbol_from(name.to_string(), kind, node));
}

/// `field_declaration` can declare multiple variables (`int a, b;`) via
/// repeated `declarator` fields — each gets its own symbol, ranged to just
/// that declarator rather than the whole (possibly multi-variable) statement.
fn push_field_declarators(node: Node, source: &[u8], out: &mut Vec<Symbol>) {
    let mut cursor = node.walk();
    for declarator in node.children_by_field_name("declarator", &mut cursor) {
        let Some(name_node) = declarator.child_by_field_name("name") else {
            continue;
        };
        let Ok(name) = name_node.utf8_text(source) else {
            continue;
        };
        out.push(symbol_from(name.to_string(), SymbolKind::Field, declarator));
    }
}

fn symbol_from(name: String, kind: SymbolKind, node: Node) -> Symbol {
    Symbol {
        name,
        kind,
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        start_byte: node.start_byte() as u32,
        end_byte: node.end_byte() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
package com.example;

public class UserService {
    private String name;
    private int age, score;

    public UserService(String name) {
        this.name = name;
    }

    public String getName() {
        return name;
    }
}

interface Greeter {
    String greet();
}

enum Status {
    ACTIVE, INACTIVE
}
"#;

    #[test]
    fn extracts_class_interface_enum_method_and_fields() {
        let facts = JavaIndexer.index(SAMPLE);
        let by_name = |name: &str| facts.symbols.iter().find(|s| s.name == name);

        let class = by_name("UserService").expect("class symbol");
        assert_eq!(class.kind, SymbolKind::Class);
        assert!(class.start_line < class.end_line, "class spans multiple lines");
        assert!(class.start_byte < class.end_byte);

        assert_eq!(by_name("Greeter").unwrap().kind, SymbolKind::Interface);
        assert_eq!(by_name("Status").unwrap().kind, SymbolKind::Enum);
        assert_eq!(by_name("getName").unwrap().kind, SymbolKind::Method);
        assert_eq!(by_name("UserService").unwrap().kind, SymbolKind::Class);

        let name_field = by_name("name").expect("field symbol");
        assert_eq!(name_field.kind, SymbolKind::Field);
        let age_field = by_name("age").expect("multi-declarator field symbol");
        assert_eq!(age_field.kind, SymbolKind::Field);
        let score_field = by_name("score").expect("multi-declarator field symbol");
        assert_ne!(age_field.start_byte, score_field.start_byte);
    }

    #[test]
    fn does_not_panic_on_malformed_input() {
        let facts = JavaIndexer.index("public class Broken {\n    public void f( {\n");
        // No panic is the assertion; a partial/empty symbol list is fine.
        let _ = facts;
    }
}
