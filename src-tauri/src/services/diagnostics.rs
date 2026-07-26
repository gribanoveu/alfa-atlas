//! Diagnostics service: computes broken references, duplicate anchors, missing
//! images, and circular includes over the `WorkspaceIndex` repositories.
//!
//! `run_all` recomputes diagnostics for every document; `run_for` recomputes
//! for a single document and its reverse-dependents (documents that include or
//! xref it). Results are written back into the index via `set_diagnostics`.

use std::collections::HashSet;

use crate::domain::workspace_index::{
    Diagnostic, DiagnosticKind, DocumentId, Severity,
};
use crate::services::workspace_index::WorkspaceIndex;

/// Recompute diagnostics for every document in the index.
pub fn run_all(index: &WorkspaceIndex) {
    let docs = index.documents_iter();
    for d in &docs {
        let diags = diagnose_one(index, &d.id);
        index.set_diagnostics(&d.id, diags);
    }
}

/// Recompute diagnostics for `doc` and every document that depends on it.
pub fn run_for(index: &WorkspaceIndex, doc: &DocumentId) {
    let mut queue: Vec<DocumentId> = vec![doc.clone()];
    let mut seen: HashSet<DocumentId> = HashSet::new();
    seen.insert(doc.clone());
    while let Some(current) = queue.pop() {
        let diags = diagnose_one(index, &current);
        index.set_diagnostics(&current, diags);
        for dep in index.dependents_of(&current) {
            if seen.insert(dep.clone()) {
                queue.push(dep);
            }
        }
    }
}

fn diagnose_one(index: &WorkspaceIndex, doc: &DocumentId) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    // Missing include.
    for inc in index.find_includes(doc) {
        if !index.document_exists_by_relative(&inc.path) {
            out.push(Diagnostic {
                kind: DiagnosticKind::MissingInclude,
                message: format!("include target not found: {}", inc.path),
                document: doc.clone(),
                line: inc.line,
                column: inc.column,
                severity: Severity::Error,
            });
        }
    }

    // Xref: missing document or missing anchor.
    for r in index.find_references(doc) {
        if r.target_document.is_empty() {
            // Pure `#anchor` reference within the same doc.
            if let Some(anchor) = &r.anchor {
                if !index.anchor_exists_in(doc, anchor) {
                    out.push(Diagnostic {
                        kind: DiagnosticKind::MissingXrefAnchor,
                        message: format!("anchor not found in document: #{}", anchor),
                        document: doc.clone(),
                        line: r.line,
                        column: r.column,
                        severity: Severity::Error,
                    });
                }
            }
            continue;
        }
        if !index.document_exists_by_relative(&r.target_document) {
            out.push(Diagnostic {
                kind: DiagnosticKind::MissingXrefDocument,
                message: format!("xref target document not found: {}", r.target_document),
                document: doc.clone(),
                line: r.line,
                column: r.column,
                severity: Severity::Error,
            });
            continue;
        }
        if let Some(anchor) = &r.anchor {
            let target_id = DocumentId::new(r.target_document.clone());
            if !index.anchor_exists_in(&target_id, anchor) {
                out.push(Diagnostic {
                    kind: DiagnosticKind::MissingXrefAnchor,
                    message: format!("anchor not found in {}: #{}", r.target_document, anchor),
                    document: doc.clone(),
                    line: r.line,
                    column: r.column,
                    severity: Severity::Error,
                });
            }
        }
    }

    // Missing image (path doesn't exist on disk under repo root).
    for img in index.images_for_doc(doc) {
        if !index.image_exists(&img.path) {
            out.push(Diagnostic {
                kind: DiagnosticKind::MissingImage,
                message: format!("image not found: {}", img.path),
                document: doc.clone(),
                line: img.line,
                column: 1,
                severity: Severity::Error,
            });
        }
    }

    // Duplicate anchor (same id defined in >1 document).
    for a in index.find_anchors(doc) {
        if index.anchor_count(&a.id) > 1 {
            out.push(Diagnostic {
                kind: DiagnosticKind::DuplicateAnchor,
                message: format!("anchor id defined more than once: {}", a.id),
                document: doc.clone(),
                line: a.line,
                column: a.column,
                severity: Severity::Warning,
            });
        }
    }

    // Circular include (DFS from this doc).
    if let Some(cycle) = detect_cycle(index, doc) {
        out.push(Diagnostic {
            kind: DiagnosticKind::CircularInclude,
            message: format!("circular include chain: {}", cycle.join(" -> ")),
            document: doc.clone(),
            line: 1,
            column: 1,
            severity: Severity::Error,
        });
    }

    out
}

/// DFS over the include graph from `start`, tracking the current chain.
/// Returns the chain as a `Vec<DocumentId>` if a cycle is found.
fn detect_cycle(index: &WorkspaceIndex, start: &DocumentId) -> Option<Vec<String>> {
    fn dfs(
        index: &WorkspaceIndex,
        current: &DocumentId,
        chain: &mut Vec<DocumentId>,
        on_chain: &mut HashSet<DocumentId>,
    ) -> Option<Vec<DocumentId>> {
        if !on_chain.insert(current.clone()) {
            // Already on chain -> cycle. Trim to the cycle itself.
            let pos = chain.iter().position(|d| d == current).unwrap_or(0);
            return Some(chain[pos..].to_vec());
        }
        chain.push(current.clone());

        for inc in index.find_includes(current) {
            let target = DocumentId::new(inc.path.clone());
            if index.document_exists_by_relative(&inc.path) {
                if let Some(cycle) = dfs(index, &target, chain, on_chain) {
                    return Some(cycle);
                }
            }
        }

        chain.pop();
        on_chain.remove(current);
        None
    }

    let mut chain = Vec::new();
    let mut on_chain = HashSet::new();
    dfs(index, start, &mut chain, &mut on_chain).map(|c| {
        c.into_iter()
            .map(|d| d.0.clone())
            .chain(std::iter::once(start.0.clone()))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::parsers::registry::ParserRegistry;
    use crate::services::workspace_index::WorkspaceIndex;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("docflow-diag-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn build(root: &std::path::Path) -> WorkspaceIndex {
        let idx = WorkspaceIndex::new(ParserRegistry::new());
        idx.build(root.to_path_buf()).unwrap();
        idx
    }

    #[test]
    fn detects_missing_include() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "include::missing.adoc[]\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags.iter().any(|d| d.kind == DiagnosticKind::MissingInclude));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_circular_include() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "include::b.adoc[]\n").unwrap();
        fs::write(root.join("b.adoc"), "include::a.adoc[]\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags.iter().any(|d| d.kind == DiagnosticKind::CircularInclude));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_self_include() {
        let root = temp_dir();
        fs::write(root.join("self.adoc"), "include::self.adoc[]\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags.iter().any(|d| d.kind == DiagnosticKind::CircularInclude));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_duplicate_anchor() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "[[dup]]\n= A\n").unwrap();
        fs::write(root.join("b.adoc"), "[[dup]]\n= B\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags.iter().any(|d| d.kind == DiagnosticKind::DuplicateAnchor));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_missing_xref_document() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "xref:nope.adoc[]\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags
            .iter()
            .any(|d| d.kind == DiagnosticKind::MissingXrefDocument));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_missing_xref_anchor() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "xref:b.adoc#missing[]\n").unwrap();
        fs::write(root.join("b.adoc"), "= B\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags
            .iter()
            .any(|d| d.kind == DiagnosticKind::MissingXrefAnchor));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_missing_image() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "image::nope.png[]\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags.iter().any(|d| d.kind == DiagnosticKind::MissingImage));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn clean_doc_has_no_diagnostics() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "[[ok]]\n= A\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags.is_empty(), "got: {:?}", diags);
        fs::remove_dir_all(&root).ok();
    }
}