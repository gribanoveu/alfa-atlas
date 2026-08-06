//! Tree-sitter-backed symbol extraction for Java.
//!
//! This is the swap point mentioned in the Repository Index design:
//! replacing tree-sitter with a JDT Language Server-backed indexer later
//! means writing `JdtLsJavaIndexer: LanguageIndexer` and changing one line
//! in `infra::language_indexers::default_indexers` — nothing else in the
//! index depends on tree-sitter specifically.

use tree_sitter::{Node, Parser};

use crate::domain::repo_index::{ImportRef, LanguageFacts, LanguageIndexer, Symbol, SymbolKind};

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
        let mut imports = Vec::new();
        walk(tree.root_node(), content.as_bytes(), &mut symbols, &mut imports);
        LanguageFacts { symbols, imports }
    }
}

fn walk(node: Node, source: &[u8], out: &mut Vec<Symbol>, imports: &mut Vec<ImportRef>) {
    match node.kind() {
        "class_declaration" => push_named(node, source, SymbolKind::Class, out),
        "interface_declaration" => push_named(node, source, SymbolKind::Interface, out),
        "enum_declaration" => push_named(node, source, SymbolKind::Enum, out),
        "method_declaration" | "constructor_declaration" => {
            push_named(node, source, SymbolKind::Method, out);
            // Deliberately does not recurse into a method/constructor's own
            // body: a local or anonymous class declared inside it (e.g. `new
            // Runnable() { public void run() { ... } }`) would otherwise
            // contribute its own nested `method_declaration`/
            // `field_declaration` nodes — flattened into this same `out`
            // list alongside every top-level symbol, with a byte range
            // *inside* the enclosing method's own range. `ChunkBuilder`'s
            // Java strategy (`spans_from_backward_gap_symbols`) assumes its
            // anchors are sequential and non-overlapping; a nested anchor
            // breaks that (its end_byte can land before the *previous*
            // anchor's end_byte), which previously crashed the whole
            // embedding-sync worker on `content[start..end]` once
            // `start > end`. Stopping here means only genuinely top-level
            // members of a type ever become anchors — anything nested
            // inside a method body is already covered by that method's own
            // (correctly non-overlapping) span.
            return;
        }
        "field_declaration" => {
            push_field_declarators(node, source, out);
            // Same reasoning as above — a field initializer can also embed
            // an anonymous class with its own methods.
            return;
        }
        "import_declaration" => push_import(node, source, imports),
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, source, out, imports);
    }
}

/// `import_declaration`'s children (per the grammar) are, in order: an
/// optional `static` keyword token, the dotted name as either an
/// `identifier` or `scoped_identifier` node (already-joined text — no
/// reconstruction needed), and an optional `asterisk` node for a wildcard
/// import (`import foo.bar.*;`). A `static` member import's name includes
/// the member (e.g. `com.foo.Bar.method`), which won't suffix-match any
/// real file — that's an accepted, silent under-resolution (see
/// `RepositoryIndex::java_dependencies`'s "no match" fallback), not
/// specially handled here.
fn push_import(node: Node, source: &[u8], out: &mut Vec<ImportRef>) {
    let mut cursor = node.walk();
    let mut fqn: Option<String> = None;
    let mut is_wildcard = false;
    for child in node.children(&mut cursor) {
        match child.kind() {
            "identifier" | "scoped_identifier" => {
                if let Ok(text) = child.utf8_text(source) {
                    fqn = Some(text.to_string());
                }
            }
            "asterisk" => is_wildcard = true,
            _ => {}
        }
    }
    if let Some(fqn) = fqn {
        out.push(ImportRef { fqn, is_wildcard });
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
    fn extracts_regular_static_and_wildcard_imports() {
        let src = r#"
import com.example.Foo;
import static com.example.Bar.method;
import com.example.util.*;

class C {}
"#;
        let facts = JavaIndexer.index(src);
        let by_fqn = |fqn: &str| facts.imports.iter().find(|i| i.fqn == fqn);

        let foo = by_fqn("com.example.Foo").expect("regular import");
        assert!(!foo.is_wildcard);

        let bar = by_fqn("com.example.Bar.method").expect("static import");
        assert!(!bar.is_wildcard);

        let util = by_fqn("com.example.util").expect("wildcard import");
        assert!(util.is_wildcard);
    }

    #[test]
    fn does_not_panic_on_malformed_input() {
        let facts = JavaIndexer.index("public class Broken {\n    public void f( {\n");
        // No panic is the assertion; a partial/empty symbol list is fine.
        let _ = facts;
    }

    /// Regression test: an anonymous class declared inside a method body
    /// (a common Mockito/Runnable/etc. pattern) used to contribute its own
    /// nested `method_declaration` as a flat top-level `Method` symbol,
    /// with a byte range *inside* the enclosing method's range — breaking
    /// `ChunkBuilder`'s assumption that anchors are sequential and
    /// non-overlapping (`domain::chunk_index::
    /// spans_from_backward_gap_symbols`) and crashing the embedding-sync
    /// worker on a `content[start..end]` slice once a later anchor's
    /// `end_byte` landed before an earlier one's.
    #[test]
    fn does_not_extract_a_nested_method_from_an_anonymous_class() {
        let src = r#"
public class Setup {
    public void configure() {
        doSomething(new Runnable() {
            @Override
            public void run() {
                System.out.println("nested");
            }
        });
    }

    public void teardown() {
    }
}
"#;
        let facts = JavaIndexer.index(src);
        let methods: Vec<&Symbol> = facts
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Method)
            .collect();

        assert!(
            !methods.iter().any(|s| s.name == "run"),
            "an anonymous class's method must not be extracted as a top-level symbol"
        );
        let configure = methods.iter().find(|s| s.name == "configure").unwrap();
        let teardown = methods.iter().find(|s| s.name == "teardown").unwrap();
        // The two real top-level methods must not overlap — `configure`
        // must fully end before `teardown` starts.
        assert!(configure.end_byte <= teardown.start_byte);
    }
}
