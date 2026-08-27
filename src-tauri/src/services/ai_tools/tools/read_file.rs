//! `readFile` — one file's text, optionally narrowed to a line range.
//!
//! An out-of-range range is clamped rather than rejected: the model is
//! guessing at line numbers from a previous search hit, and a hard error
//! would cost a round trip to learn the file got shorter.

use std::fs;

use crate::domain::ai_tools::{ReadFileArgs, ToolError, ToolScope};
use crate::domain::llm::LlmToolDefinition;

use super::super::resolve::resolve_existing_path;

/// One `readFile` result: a possibly-partial slice of a file's lines,
/// along with enough range/total metadata for the model to know it's
/// looking at less than the whole file.
pub(super) struct FileSlice {
    pub(super) content: String,
    pub(super) start_line: u32,
    pub(super) end_line: u32,
    pub(super) total_lines: u32,
}

pub(super) fn read_file(scope: &ToolScope, args: ReadFileArgs) -> Result<FileSlice, ToolError> {
    // No extension filtering here, unlike `docs_fs::read_project_file` —
    // the tool boundary is containment under `scope.root` alone. In
    // `FullRepo` mode the harness must be able to read source files, which
    // aren't in `is_supported_file`'s doc-format list.
    let canonical = resolve_existing_path(scope, &args.path)?;
    if !canonical.is_file() {
        return Err(ToolError::NotAFile(args.path));
    }
    let content = fs::read_to_string(&canonical).map_err(ToolError::Io)?;
    Ok(slice_lines(content, args.start_line, args.end_line))
}

/// Clamps `start_line`/`end_line` into range rather than erroring (mirrors
/// `SemanticSearchArgs.top_k`'s `.clamp(1, MAX_TOP_K)` handling below).
/// When neither is requested, `content` is returned byte-identical to what
/// `fs::read_to_string` produced — no split/rejoin round trip for the
/// common full-file case. An empty file reports `start_line: 0,
/// end_line: 0, total_lines: 0` (there is no line 1 to claim). If
/// `end_line` clamps below `start_line` after each is independently
/// clamped into `[1, total_lines]`, `end_line` is raised to `start_line`
/// (returns that one line) rather than erroring.
pub(super) fn slice_lines(content: String, start_line: Option<u32>, end_line: Option<u32>) -> FileSlice {
    if start_line.is_none() && end_line.is_none() {
        let total_lines = content.lines().count() as u32;
        let start_line = if total_lines == 0 { 0 } else { 1 };
        return FileSlice { content, start_line, end_line: total_lines, total_lines };
    }
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len() as u32;
    if total_lines == 0 {
        return FileSlice { content: String::new(), start_line: 0, end_line: 0, total_lines: 0 };
    }
    let start = start_line.unwrap_or(1).clamp(1, total_lines);
    let end = end_line.unwrap_or(total_lines).clamp(start, total_lines);
    let mut sliced = lines[(start - 1) as usize..end as usize].join("\n");
    sliced.push('\n');
    FileSlice { content: sliced, start_line: start, end_line: end, total_lines }
}

/// The `readFile` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "readFile".to_string(),
        description: "Read the text content of one file by its path relative to the current access-mode root (documentation root in Docs-only mode, repository root in Full-repo mode), optionally restricted to a line range. Use when the relevant file is already known — especially paths from semanticSearch/grep results. For \"how does X work\" questions: after search, read at most 2–3 files first — the matching .adoc (if any) and the owning implementation (*Service / handler named by the doc or operation), not mappers, DTOs, or sibling services until the algorithm is incomplete. Prefer a line range for a large file when only part of it is relevant. Paths returned by grep/semanticSearch, and paths constructed from listFiles entries (excluding the tree's display-only root label), are already correctly rooted — pass them here as-is, with no manual prefix added or stripped."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to the current access-mode root (documentation root in Docs-only mode, repository root in Full-repo mode)."
                },
                "startLine": {
                    "type": ["integer", "null"],
                    "minimum": 1,
                    "description": "1-indexed first line to return (inclusive). Omit or null to start from the beginning of the file."
                },
                "endLine": {
                    "type": ["integer", "null"],
                    "minimum": 1,
                    "description": "1-indexed last line to return (inclusive). Omit or null to read through the end of the file. Out-of-range values are clamped, not rejected."
                }
            },
            "required": ["path"]
        }),
        }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::domain::ai_access::AiAccessMode;
    use crate::domain::ai_tools::{ToolError, ToolResult, ToolScope};
    use crate::services::ai_tools::testing::*;

    #[test]
    fn read_file_inside_docs_root_succeeds_in_docs_only() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let content = read(&scope, "intro.adoc").unwrap();
        assert_eq!(content, "= Intro\n");
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_on_a_directory_returns_not_a_file() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = read(&scope, ".").unwrap_err();
        assert!(matches!(err, ToolError::NotAFile(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_rejects_parent_escape_in_both_modes() {
        let (repo, docs) = fixture_repo();

        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let err = read(&docs_only, "../src/main.rs").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let err = read(&full_repo, "../outside.txt").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    /// Regression test: a model guessing at a plausible-but-wrong nested
    /// path (e.g. `components/schemas/all.yaml` in a repo whose schemas
    /// actually live directly at `schemas/all.yaml`) must see a clean
    /// `NotFound` it can react to — not `ToolError::Io` wrapping a raw
    /// `"No such file or directory (os error 2)"` with no path in it, which
    /// is what `paths::ensure_under` used to surface whenever *more* than
    /// the immediate parent directory of a `readFile`/`listFiles` path was
    /// missing.
    #[test]
    fn read_file_missing_several_directories_deep_returns_clean_not_found() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = read(&scope, "components/schemas/all.yaml").unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)), "expected NotFound, got {err:?}");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_same_relative_path_resolves_against_different_roots_by_mode() {
        let (repo, docs) = fixture_repo();

        // "src/main.rs" only exists under `repo`, not under `docs` — so the
        // same relative path is simply absent from the docs-only root
        // (there is no `docs/src/main.rs`), while it resolves fine once the
        // scope root widens to the whole repo. This is `ToolScope`'s mode
        // switch doing its job, not a `..`-escape (that's covered above).
        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        assert!(read(&docs_only, "src/main.rs").is_err());

        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let content = read(&full_repo, "src/main.rs").unwrap();
        assert_eq!(content, "fn main() {}\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_full_repo_accepts_docs_relative_path_when_that_file_exists() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("updateTransactionSpecifics")).unwrap();
        fs::write(
            docs.join("updateTransactionSpecifics/foo.adoc"),
            "= Foo\n",
        )
        .unwrap();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let content = read(&full_repo, "updateTransactionSpecifics/foo.adoc").unwrap();
        assert_eq!(content, "= Foo\n");

        let listed = list(&full_repo, Some("updateTransactionSpecifics")).unwrap();
        assert!(listed.iter().any(|e| e.path.ends_with("foo.adoc")));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_docs_only_strips_docs_root_prefix_when_that_file_exists() {
        let (repo, docs) = fixture_repo();
        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        assert_eq!(read(&docs_only, "docs/intro.adoc").unwrap(), "= Intro\n");
        assert_eq!(read(&docs_only, "intro.adoc").unwrap(), "= Intro\n");

        let err = read(&docs_only, "docs/missing.adoc").unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_docs_only_strips_nested_docs_prefix() {
        let repo = fixture_dir("nested-docs-");
        let docs = repo.join("src/docs/asciidoc");
        fs::create_dir_all(docs.join("method")).unwrap();
        fs::write(docs.join("method/foo.adoc"), "= Foo\n").unwrap();
        let repo = repo.canonicalize().unwrap();
        let docs = docs.canonicalize().unwrap();
        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        assert_eq!(
            read(&docs_only, "src/docs/asciidoc/method/foo.adoc").unwrap(),
            "= Foo\n"
        );
        assert_eq!(read(&docs_only, "asciidoc/method/foo.adoc").unwrap(), "= Foo\n");
        assert_eq!(read(&docs_only, "method/foo.adoc").unwrap(), "= Foo\n");

        fs::remove_dir_all(&repo).ok();
    }

    /// `join_relative`'s `..`-rejection only catches lexical traversal; the
    /// real defense against a path that resolves outside the root by other
    /// means (e.g. a symlink) is `ensure_under`'s canonicalize+`starts_with`
    /// check. Exercise that directly so the containment guarantee isn't
    /// only proven for the lexical case.
    #[cfg(unix)]
    #[test]
    fn read_file_rejects_symlink_escaping_docs_root() {
        let (repo, docs) = fixture_repo();
        std::os::unix::fs::symlink(repo.join("src/main.rs"), docs.join("leak.adoc")).unwrap();

        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let err = read(&docs_only, "leak.adoc").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_full_read_reports_total_lines_and_is_byte_identical() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "intro.adoc", None, None).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "= Intro\n");
                assert_eq!((start_line, end_line, total_lines), (1, 1, 1));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_returns_a_requested_line_range() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("multi.adoc"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "multi.adoc", Some(2), Some(4)).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "two\nthree\nfour\n");
                assert_eq!((start_line, end_line, total_lines), (2, 4, 5));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_clamps_out_of_range_start_and_end_line() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("multi.adoc"), "one\ntwo\nthree\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "multi.adoc", Some(0), Some(9999)).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "one\ntwo\nthree\n");
                assert_eq!((start_line, end_line, total_lines), (1, 3, 3));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_clamps_end_line_below_start_line_up_to_start_line() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("multi.adoc"), "one\ntwo\nthree\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "multi.adoc", Some(3), Some(1)).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "three\n");
                assert_eq!((start_line, end_line, total_lines), (3, 3, 3));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_line_range_on_empty_file_reports_zero_lines() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("empty.adoc"), "").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        match read_range(&scope, "empty.adoc", Some(1), Some(5)).unwrap() {
            ToolResult::File { content, start_line, end_line, total_lines } => {
                assert_eq!(content, "");
                assert_eq!((start_line, end_line, total_lines), (0, 0, 0));
            }
            other => panic!("expected ToolResult::File, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }
}
