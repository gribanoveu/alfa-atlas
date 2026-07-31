//! Rewrites `include::`/`image::`/`xref:`/`<<...>>` references across the
//! project when a document (or a whole directory of documents) is renamed or
//! moved, so they keep pointing at the right file instead of silently
//! breaking. `WorkspaceIndex::rename_document` deliberately leaves references
//! in other documents pointing at the old path — this module is what
//! actually fixes them.
//!
//! Line/column positions stored in the index can't be trusted for the exact
//! text splice (images have no column at all, and production AsciiDoc
//! include/xref columns are either hardcoded or point at the macro start,
//! not the target substring — see the domain types). So this only uses the
//! index to find *which file and which line* to look at, then re-matches
//! the macro target fresh against that line's current text before touching
//! anything — a lightweight, self-correcting safety check.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use thiserror::Error;

use crate::domain::workspace_index::{relativize, resolve_against_document, DocumentId};
use crate::services::workspace_index::WorkspaceIndex;

#[derive(Debug, Error)]
pub enum ReferenceRewriteError {
    #[error("failed to read {0}: {1}")]
    Read(String, #[source] std::io::Error),
    #[error("failed to write {0}: {1}")]
    Write(String, #[source] std::io::Error),
}

/// One document that moved: both repo-relative keys.
#[derive(Debug, Clone)]
pub struct RenamedPath {
    pub old: String,
    pub new: String,
}

/// One file that was patched, and how many references inside it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewrittenFile {
    pub repo_relative_path: String,
    pub count: u32,
}

static INCLUDE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"include::([^\s\[]+)\[").expect("valid regex"));
static IMAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"image::([^\s\[]+)\[").expect("valid regex"));
static XREF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"xref:([^\s\[]+)\[").expect("valid regex"));
static ANCHOR_XREF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<<([^,>]+)(?:,[^>]*)?>>").expect("valid regex"));

fn split_anchor(target: &str) -> (&str, Option<&str>) {
    match target.split_once('#') {
        Some((path, anchor)) => (path, Some(anchor)),
        None => (target, None),
    }
}

struct MacroMatch {
    start: usize,
    end: usize,
    target: String,
    anchor: Option<String>,
}

/// Finds every `include::`/`image::`/`xref:`/`<<...>>` target on one line,
/// regardless of which macro it belongs to.
fn find_macro_matches(line: &str) -> Vec<MacroMatch> {
    let mut matches = Vec::new();
    for re in [&*INCLUDE_RE, &*IMAGE_RE, &*XREF_RE] {
        for caps in re.captures_iter(line) {
            let m = caps.get(1).expect("group 1 always present on match");
            let (path, anchor) = split_anchor(m.as_str());
            if path.is_empty() {
                continue;
            }
            matches.push(MacroMatch {
                start: m.start(),
                end: m.end(),
                target: path.to_string(),
                anchor: anchor.map(str::to_string),
            });
        }
    }
    for caps in ANCHOR_XREF_RE.captures_iter(line) {
        let m = caps.get(1).expect("group 1 always present on match");
        let (path, anchor) = split_anchor(m.as_str());
        // A same-document anchor like `<<some-anchor>>` has no path part —
        // nothing to rewrite when an unrelated file moves.
        if path.is_empty() {
            continue;
        }
        matches.push(MacroMatch {
            start: m.start(),
            end: m.end(),
            target: path.to_string(),
            anchor: anchor.map(str::to_string),
        });
    }
    matches
}

/// Rewrites every macro target on `line` that currently resolves (relative
/// to `source_document`) to `old_target`, replacing it with `new_relative`.
/// Returns `None` if nothing on the line matched — the common case, since
/// most lines aren't the one containing the reference.
fn rewrite_line(
    line: &str,
    source_document: &str,
    old_target: &str,
    new_relative: &str,
) -> Option<String> {
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    for m in find_macro_matches(line) {
        if resolve_against_document(source_document, &m.target) == old_target {
            let mut replacement = new_relative.to_string();
            if let Some(anchor) = &m.anchor {
                replacement.push('#');
                replacement.push_str(anchor);
            }
            replacements.push((m.start, m.end, replacement));
        }
    }
    if replacements.is_empty() {
        return None;
    }

    let mut result = String::with_capacity(line.len());
    let mut last = 0;
    for (start, end, replacement) in &replacements {
        result.push_str(&line[last..*start]);
        result.push_str(replacement);
        last = *end;
    }
    result.push_str(&line[last..]);
    Some(result)
}

/// Every `(document, line)` that currently references `old_path`, combining
/// the reverse-dependency map (includes/xrefs) with the direct by-path image
/// lookup.
fn find_referencing_lines(index: &WorkspaceIndex, old_path: &str) -> Vec<(DocumentId, u32)> {
    let old_id = DocumentId::new(old_path.to_string());
    let mut hits = Vec::new();

    for dependent in index.dependents_of(&old_id) {
        for inc in index.find_includes(&dependent) {
            if inc.path == old_path {
                hits.push((dependent.clone(), inc.line));
            }
        }
        for r in index.find_references(&dependent) {
            if r.target_document == old_path {
                hits.push((dependent.clone(), r.line));
            }
        }
    }
    for img in index.find_image(old_path) {
        hits.push((img.document, img.line));
    }
    hits
}

/// Expands a directory move into one `RenamedPath` per document that was
/// under it, so the same per-file rewrite logic below handles a folder
/// rename exactly like N simultaneous file renames.
pub fn renamed_paths_for_dir_move(
    index: &WorkspaceIndex,
    old_prefix: &str,
    new_prefix: &str,
) -> Vec<RenamedPath> {
    let prefix_with_slash = format!("{old_prefix}/");
    index
        .documents_iter()
        .into_iter()
        .filter_map(|doc| {
            let old = doc.id.0;
            if old == old_prefix {
                Some(RenamedPath {
                    new: new_prefix.to_string(),
                    old,
                })
            } else if let Some(suffix) = old.strip_prefix(&prefix_with_slash) {
                Some(RenamedPath {
                    new: format!("{new_prefix}/{suffix}"),
                    old,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Applies every reference rewrite implied by `renamed` (one pair for a
/// single file rename, or the output of `renamed_paths_for_dir_move` for a
/// directory move) and writes each touched file exactly once, even if it
/// referenced several of the moved documents.
///
/// Must be called against the index's state *before* the actual filesystem
/// rename/move happens — it looks up "who currently references the old
/// path," which only makes sense pre-move.
///
/// Note: normalizes line endings to `\n` and ensures a trailing newline on
/// any file it rewrites, matching this project's own documents (LF); it
/// does not attempt to preserve CRLF line endings.
pub fn rewrite_references(
    index: &WorkspaceIndex,
    repo_root: &Path,
    renamed: &[RenamedPath],
) -> Result<Vec<RewrittenFile>, ReferenceRewriteError> {
    // For a directory move, a referencing document can itself be one of the
    // documents that moved (e.g. two files in the same moved folder that
    // reference each other). The replacement text has to be computed as if
    // written from that document's *new* location — even though we still
    // read/write the file at its current (pre-move) path below, since the
    // actual `fs::rename` of the whole tree hasn't happened yet — otherwise
    // a same-folder reference between two co-moving files would incorrectly
    // grow a `../` prefix instead of staying untouched.
    let old_to_new: HashMap<&str, &str> =
        renamed.iter().map(|p| (p.old.as_str(), p.new.as_str())).collect();

    let mut files: HashMap<DocumentId, (PathBuf, Vec<String>, u32)> = HashMap::new();

    for pair in renamed {
        for (doc_id, line_no) in find_referencing_lines(index, &pair.old) {
            if !files.contains_key(&doc_id) {
                let abs = repo_root.join(&doc_id.0);
                let content = fs::read_to_string(&abs)
                    .map_err(|e| ReferenceRewriteError::Read(doc_id.0.clone(), e))?;
                let lines: Vec<String> = content.lines().map(str::to_string).collect();
                files.insert(doc_id.clone(), (abs, lines, 0));
            }
            let entry = files.get_mut(&doc_id).expect("just inserted above");
            let idx = (line_no as usize).saturating_sub(1);
            if let Some(line) = entry.1.get(idx) {
                let effective_source = old_to_new.get(doc_id.0.as_str()).copied().unwrap_or(&doc_id.0);
                let new_relative = relativize(effective_source, &pair.new);
                if let Some(rewritten) = rewrite_line(line, &doc_id.0, &pair.old, &new_relative) {
                    // A directory move can resolve to a byte-identical
                    // replacement (e.g. two files in the same folder that
                    // move together — the relative path between them never
                    // changes). Don't count or persist a no-op.
                    if rewritten != *line {
                        entry.1[idx] = rewritten;
                        entry.2 += 1;
                    }
                }
            }
        }
    }

    let mut results = Vec::new();
    for (doc_id, (abs_path, lines, count)) in files {
        if count == 0 {
            continue;
        }
        let mut content = lines.join("\n");
        content.push('\n');
        fs::write(&abs_path, content).map_err(|e| ReferenceRewriteError::Write(doc_id.0.clone(), e))?;
        results.push(RewrittenFile {
            repo_relative_path: doc_id.0,
            count,
        });
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_line_updates_include_target() {
        let line = "include::createPatentNotification.puml[]";
        let rewritten = rewrite_line(
            line,
            "createPatentNotification/createPatentNotification.adoc",
            "createPatentNotification/createPatentNotification.puml",
            "renamed.puml",
        );
        assert_eq!(
            rewritten.as_deref(),
            Some("include::renamed.puml[]")
        );
    }

    #[test]
    fn rewrite_line_updates_image_target() {
        let line = "image::diagrams/old.png[Alt text]";
        let rewritten = rewrite_line(
            line,
            "docs/foo.adoc",
            "docs/diagrams/old.png",
            "diagrams/new.png",
        );
        assert_eq!(
            rewritten.as_deref(),
            Some("image::diagrams/new.png[Alt text]")
        );
    }

    #[test]
    fn rewrite_line_updates_xref_and_keeps_anchor() {
        let line = "See xref:../index.adoc#common-headers[headers].";
        let rewritten = rewrite_line(line, "a/b/doc.adoc", "a/index.adoc", "../../index.adoc");
        assert_eq!(
            rewritten.as_deref(),
            Some("See xref:../../index.adoc#common-headers[headers].")
        );
    }

    #[test]
    fn rewrite_line_updates_double_angle_xref() {
        let line = "See <<../index.adoc#common-headers,общие заголовки>>.";
        let rewritten = rewrite_line(line, "a/b/doc.adoc", "a/index.adoc", "../../index.adoc");
        assert_eq!(
            rewritten.as_deref(),
            Some("See <<../../index.adoc#common-headers,общие заголовки>>.")
        );
    }

    #[test]
    fn rewrite_line_ignores_unrelated_targets() {
        let line = "include::other.adoc[]";
        let rewritten = rewrite_line(line, "docs/foo.adoc", "docs/moved.adoc", "elsewhere.adoc");
        assert_eq!(rewritten, None);
    }

    #[test]
    fn rewrite_line_ignores_same_document_anchor() {
        let line = "See <<some-anchor>> above.";
        let rewritten = rewrite_line(line, "docs/foo.adoc", "docs/moved.adoc", "elsewhere.adoc");
        assert_eq!(rewritten, None);
    }

    #[test]
    fn rewrite_line_handles_two_references_on_one_line() {
        let line = "<<a.adoc,A>> and <<b.adoc,B>>";
        let rewritten = rewrite_line(line, "docs/x.adoc", "docs/a.adoc", "renamed-a.adoc");
        assert_eq!(
            rewritten.as_deref(),
            Some("<<renamed-a.adoc,A>> and <<b.adoc,B>>")
        );
    }

    // --- rewrite_references (integration-style, against a real WorkspaceIndex + temp dir) ---

    fn temp_dir() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("alfa-atlas-ref-rewrite-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn build_index(root: &Path) -> std::sync::Arc<WorkspaceIndex> {
        let idx = std::sync::Arc::new(WorkspaceIndex::new(
            crate::infra::parsers::registry::ParserRegistry::new(),
        ));
        idx.build(root.to_path_buf()).unwrap();
        idx
    }

    #[test]
    fn dir_move_keeps_same_folder_reference_untouched() {
        // Regression test: main.adoc and detail.puml both live in
        // `folder/` and move together to `moved/folder/`. main.adoc's
        // reference to detail.puml is same-folder and must stay untouched
        // (previously it incorrectly grew a `../` prefix, because the
        // replacement was computed from main.adoc's *old* location instead
        // of the new one it's also moving to).
        let root = temp_dir();
        fs::create_dir_all(root.join("folder")).unwrap();
        fs::write(
            root.join("folder").join("main.adoc"),
            "= Main\n\ninclude::detail.puml[]\n",
        )
        .unwrap();
        fs::write(root.join("folder").join("detail.puml"), "@startuml\n@enduml\n").unwrap();
        fs::write(
            root.join("outer.adoc"),
            "= Outer\n\ninclude::folder/detail.puml[]\n",
        )
        .unwrap();

        let index = build_index(&root);
        let renamed = renamed_paths_for_dir_move(&index, "folder", "moved/folder");
        let rewritten = rewrite_references(&index, &root, &renamed).unwrap();

        let main_content =
            fs::read_to_string(root.join("folder").join("main.adoc")).unwrap();
        assert_eq!(main_content, "= Main\n\ninclude::detail.puml[]\n");

        let outer_content = fs::read_to_string(root.join("outer.adoc")).unwrap();
        assert_eq!(outer_content, "= Outer\n\ninclude::moved/folder/detail.puml[]\n");

        let outer_entry = rewritten
            .iter()
            .find(|f| f.repo_relative_path == "outer.adoc")
            .expect("outer.adoc should be reported as rewritten");
        assert_eq!(outer_entry.count, 1);
        assert!(
            rewritten.iter().all(|f| f.repo_relative_path != "folder/main.adoc"),
            "main.adoc's same-folder reference needed no change, so it must not be reported as rewritten"
        );

        fs::remove_dir_all(&root).ok();
    }
}
