//! `grep` — exhaustive literal/regex line matching, for when the model
//! needs every occurrence rather than the best few `semanticSearch` ranks.

use crate::domain::ai_tools::{GrepArgs, ToolError, ToolResult, ToolScope};
use crate::domain::llm::LlmToolDefinition;
use crate::services::docs_search;

use super::super::resolve::{relative_under_maybe_missing, resolve_existing_path};

/// Exact regex content search under `scope.root` — delegates to
/// `services::docs_search::search_under_root` (shared with the user-facing
/// `docs_search` IPC). Paths in results are scope-root-relative so they
/// round-trip into `readFile`.
pub(super) fn grep(scope: &ToolScope, mut args: GrepArgs) -> Result<ToolResult, ToolError> {
    if let Some(path) = args.path.as_deref().filter(|p| !p.is_empty() && *p != ".") {
        let canonical = resolve_existing_path(scope, path)?;
        let rel = relative_under_maybe_missing(&scope.root, &canonical)?;
        args.path = Some(if rel == "." { String::new() } else { rel });
    }
    let payload = docs_search::search_under_root(&scope.root, &args)?;
    Ok(ToolResult::GrepResults {
        matches: payload.matches,
        truncated: payload.truncated,
    })
}

/// The `grep` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "grep".to_string(),
        description:
            "Exact regex search over file contents under the current access-mode root (documentation root in Docs-only mode, repository root in Full-repo mode). Secondary tool — do not use as the first search step; call semanticSearch first for discovery. Use grep only when semanticSearch is insufficient: you need every call site of a symbol, every occurrence of a literal string, or a regex pattern across files, and you already know what to match. Not for conceptual or exploratory search. Returns line-oriented hits (path, 1-indexed line, line text), capped and truncated when the limit is hit. Honors .gitignore; skips binary and oversized files. `path` may be a file (a semanticSearch hit is valid) or a subdirectory; omit it to search the whole root. Returned paths are already relative to the same root readFile uses — pass them to readFile unchanged."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Rust regex pattern (no backreferences). Case-sensitive unless caseInsensitive is true."
                },
                "path": {
                    "type": ["string", "null"],
                    "description": "Optional file or subdirectory relative to the current access-mode root. A path from semanticSearch is valid. Omit or null to search the whole root."
                },
                "glob": {
                    "type": ["string", "null"],
                    "description": "Optional filename-only glob (e.g. \"*.java\") to restrict which files are searched."
                },
                "caseInsensitive": {
                    "type": ["boolean", "null"],
                    "description": "When true, match case-insensitively. Default false."
                },
                "maxResults": {
                    "type": ["integer", "null"],
                    "minimum": 1,
                    "description": "Max number of line hits to return, default 50, capped at 200."
                }
            },
            "required": ["pattern"]
        }),
        }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::domain::ai_access::AiAccessMode;
    use crate::domain::ai_tools::{GrepArgs, ToolCall, ToolError, ToolResult, ToolScope};
    use crate::services::ai_tools::testing::*;
    use crate::services::ai_tools::{EmbeddingDeps, execute_tool};

    #[test]
    fn grep_finds_line_hits_under_docs_root_and_rejects_invalid_regex() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("guide.adoc"), "= Guide\ncall Needle.here()\nmore\n").unwrap();
        fs::write(repo.join("src/main.rs"), "fn Needle() {}\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let result = execute_tool(
            &scope,
            ToolCall::Grep(GrepArgs {
                pattern: "Needle".to_string(),
                path: None,
                glob: None,
                case_insensitive: None,
                max_results: None,
            }),
            &EmbeddingDeps::empty(),
            &[],
        )
        .unwrap();
        match result {
            ToolResult::GrepResults { matches, truncated } => {
                assert!(!truncated);
                assert_eq!(matches.len(), 1);
                assert_eq!(matches[0].path, "guide.adoc");
                assert_eq!(matches[0].line, 2);
                assert!(matches[0].text.contains("Needle"));
            }
            other => panic!("expected GrepResults, got {other:?}"),
        }

        let err = execute_tool(
            &scope,
            ToolCall::Grep(GrepArgs {
                pattern: "(unclosed".to_string(),
                path: None,
                glob: None,
                case_insensitive: None,
                max_results: None,
            }),
            &EmbeddingDeps::empty(),
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::InvalidPattern(_)), "got {err:?}");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn grep_truncates_when_max_results_is_hit() {
        let (repo, docs) = fixture_repo();
        let mut body = String::new();
        for i in 0..10 {
            body.push_str(&format!("hit {i}\n"));
        }
        fs::write(docs.join("many.adoc"), body).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let result = execute_tool(
            &scope,
            ToolCall::Grep(GrepArgs {
                pattern: "hit".to_string(),
                path: None,
                glob: None,
                case_insensitive: None,
                max_results: Some(3),
            }),
            &EmbeddingDeps::empty(),
            &[],
        )
        .unwrap();
        match result {
            ToolResult::GrepResults { matches, truncated } => {
                assert!(truncated);
                assert_eq!(matches.len(), 3);
            }
            other => panic!("expected GrepResults, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn grep_path_may_be_a_file() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("a.adoc"), "Needle in a\n").unwrap();
        fs::write(docs.join("b.adoc"), "Needle in b\n").unwrap();
        fs::write(repo.join("src/Controller.java"), "class Needle {}\n").unwrap();

        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let result = execute_tool(
            &docs_only,
            ToolCall::Grep(GrepArgs {
                pattern: "Needle".to_string(),
                path: Some("a.adoc".to_string()),
                glob: None,
                case_insensitive: None,
                max_results: None,
            }),
            &EmbeddingDeps::empty(),
            &[],
        )
        .unwrap();
        match result {
            ToolResult::GrepResults { matches, truncated } => {
                assert!(!truncated);
                assert_eq!(matches.len(), 1);
                assert_eq!(matches[0].path, "a.adoc");
            }
            other => panic!("expected GrepResults, got {other:?}"),
        }

        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let result = execute_tool(
            &full_repo,
            ToolCall::Grep(GrepArgs {
                pattern: "Needle".to_string(),
                path: Some("src/Controller.java".to_string()),
                glob: None,
                case_insensitive: None,
                max_results: None,
            }),
            &EmbeddingDeps::empty(),
            &[],
        )
        .unwrap();
        match result {
            ToolResult::GrepResults { matches, truncated } => {
                assert!(!truncated);
                assert_eq!(matches.len(), 1);
                assert_eq!(matches[0].path, "src/Controller.java");
            }
            other => panic!("expected GrepResults, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn grep_full_repo_accepts_docs_relative_file_path() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("guide.adoc"), "checkCustomerAccess here\n").unwrap();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let result = execute_tool(
            &full_repo,
            ToolCall::Grep(GrepArgs {
                pattern: "checkCustomerAccess".to_string(),
                path: Some("guide.adoc".to_string()),
                glob: None,
                case_insensitive: None,
                max_results: None,
            }),
            &EmbeddingDeps::empty(),
            &[],
        )
        .unwrap();
        match result {
            ToolResult::GrepResults { matches, truncated } => {
                assert!(!truncated);
                assert_eq!(matches.len(), 1);
                assert_eq!(matches[0].path, "docs/guide.adoc");
            }
            other => panic!("expected GrepResults, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }
}
