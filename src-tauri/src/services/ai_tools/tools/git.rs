//! `gitDiff` and `gitBlame` — read-only history, scoped like every other
//! path-taking tool.

use std::fs;

use crate::domain::ai_tools::{
    FileDiffStats, GitBlameArgs, GitDiffArgs, MAX_STATUS_ENTRIES, ToolError, ToolResult, ToolScope,
};
use crate::domain::git::{GitDiffScope, GitFileStatus};
use crate::domain::llm::LlmToolDefinition;
use crate::domain::paths;
use crate::services::{git_ops, text_diff};

use super::super::resolve::resolve_repo_relative_path;

/// Cap on how many lines a single `gitBlame` call may cover — keeps the
/// tool-message payload bounded for large files. Ranges past this are
/// clamped and flagged `truncated: true`.
pub(super) const MAX_BLAME_LINES: u32 = 400;

pub(super) fn git_diff(scope: &ToolScope, args: GitDiffArgs) -> Result<ToolResult, ToolError> {
    let repo_rel = resolve_repo_relative_path(scope, &args.path)?;
    // A directory reads as "no blob anywhere" and would come back as a valid
    // empty diff — which the model reads as "nothing changed here" and stops
    // looking. Say what actually happened instead.
    if scope.repo_root.join(&repo_rel).is_dir() {
        return Err(ToolError::InvalidArguments {
            tool: "gitDiff".into(),
            reason: format!(
                "«{}» — это каталог; gitDiff работает с одним файлом. Найди изменившиеся файлы через listFiles/grep и запроси diff по каждому.",
                args.path
            ),
        });
    }
    let repo_root = scope.repo_root.to_string_lossy();

    let file_diff = if let Some(commit) = args.commit.as_deref().filter(|c| !c.is_empty()) {
        git_ops::commit_file_diff(&repo_root, commit, &repo_rel)?
    } else {
        let diff_scope = match args.scope.as_deref() {
            None | Some("unstaged") => GitDiffScope::Unstaged,
            Some("staged") => GitDiffScope::Staged,
            Some(other) => {
                return Err(ToolError::InvalidArguments {
                    tool: "gitDiff".into(),
                    reason: format!(
                        "scope must be \"unstaged\" or \"staged\" (got \"{other}\")"
                    ),
                });
            }
        };
        git_ops::file_diff(&repo_root, &repo_rel, diff_scope)?
    };

    let label = format!("{} → {}", file_diff.original_label, file_diff.modified_label);
    let diff = if file_diff.is_binary {
        FileDiffStats {
            lines_added: 0,
            lines_removed: 0,
            unified_diff: String::new(),
            truncated: false,
        }
    } else {
        text_diff::diff_stats(&file_diff.original, &file_diff.modified)
    };

    Ok(ToolResult::GitDiff {
        path: args.path,
        label,
        diff,
        is_binary: file_diff.is_binary,
    })
}

/// `gitStatus` — the working tree's changed paths, scoped and capped.
///
/// The one thing `gitDiff` cannot answer: it takes a single file, so
/// without this the model has no way to find out *which* files changed and
/// falls back to probing directory paths (which used to come back as a
/// perfectly valid empty diff — see `git_diff`).
///
/// Paths are re-based onto the access-mode root like every other tool's,
/// and anything outside it is dropped rather than renamed: in DocsOnly a
/// change under `src/` is not something this mode is allowed to report.
pub(super) fn git_status(scope: &ToolScope) -> Result<ToolResult, ToolError> {
    let snapshot = git_ops::status(&scope.repo_root.to_string_lossy())?;

    // `relative_to` answers "." when the two are the same directory, which
    // is the FullRepo case — there every repo path is already in scope.
    let root_rel = paths::relative_to(&scope.repo_root, &scope.root)?;
    let prefix = if root_rel == "." { String::new() } else { format!("{root_rel}/") };
    let rebase = |files: Vec<GitFileStatus>| -> Vec<GitFileStatus> {
        files
            .into_iter()
            .filter_map(|f| {
                let path = f.path.strip_prefix(&prefix)?;
                Some(GitFileStatus { path: path.to_string(), status: f.status })
            })
            .collect()
    };

    let conflicted = rebase(snapshot.conflicted);
    let staged = rebase(snapshot.staged);
    let mut unstaged = rebase(snapshot.unstaged);

    // Conflicts and staged work are the smaller, more decision-relevant
    // lists, so the cap eats into `unstaged` (which is what a repo full of
    // untracked files inflates) rather than trimming all three evenly.
    let kept = conflicted.len() + staged.len();
    let truncated = kept + unstaged.len() > MAX_STATUS_ENTRIES;
    unstaged.truncate(MAX_STATUS_ENTRIES.saturating_sub(kept));

    Ok(ToolResult::GitStatus {
        branch: snapshot.branch,
        staged,
        unstaged,
        conflicted,
        truncated,
    })
}

pub(super) fn git_blame(scope: &ToolScope, args: GitBlameArgs) -> Result<ToolResult, ToolError> {
    let joined = paths::join_relative(&scope.root, &args.path)?;
    let canonical = paths::ensure_under(&scope.root, &joined)?;
    let repo_rel = paths::relative_to_lenient(&scope.repo_root, &canonical)?.replace('\\', "/");
    let repo_root = scope.repo_root.to_string_lossy();

    let start = args.start_line.unwrap_or(1).max(1);
    let file_lines = fs::read_to_string(&canonical)
        .map(|s| s.lines().count() as u32)
        .unwrap_or(0);

    let (end, truncated) = match args.end_line {
        Some(e) => {
            let e = e.max(start);
            if e - start + 1 > MAX_BLAME_LINES {
                (start + MAX_BLAME_LINES - 1, true)
            } else {
                (e, false)
            }
        }
        None => {
            let capped = start + MAX_BLAME_LINES - 1;
            if file_lines == 0 {
                (capped, false)
            } else if file_lines > capped {
                (capped, true)
            } else {
                (file_lines.max(start), false)
            }
        }
    };

    let hunks = git_ops::blame(&repo_root, &repo_rel, Some(start), Some(end))
        .map_err(|e| blame_error(&args.path, e))?;
    Ok(ToolResult::GitBlame {
        path: args.path,
        hunks,
        truncated,
    })
}

/// libgit2 answers a blame on a never-committed file with "the path '…'
/// does not exist in the given tree" — true about the tree, and useless to
/// the caller: nothing in it says the file is simply new, so the natural
/// reading is that the path was wrong.
///
/// Rewritten here, at the tool boundary, rather than in `domain::git::
/// GitError`: that type's `Display` text is matched on verbatim by
/// `src/lib/gitErrors.ts` to translate git failures for the user, and
/// editing it would silently break those translations.
fn blame_error(path: &str, err: crate::domain::git::GitError) -> ToolError {
    if err.to_string().contains("does not exist in the given tree") {
        return ToolError::Git(format!(
            "у файла «{path}» ещё нет истории — он не закоммичен, поэтому blame по нему невозможен. Его текущее содержимое целиком показывает gitDiff."
        ));
    }
    ToolError::from(err)
}

/// The `gitDiff` schema the model sees.
pub(super) fn diff_definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "gitDiff".to_string(),
        description:
            "Show the git diff for one file — recent local changes (unstaged working-tree vs index/HEAD, or staged index vs HEAD) or the change introduced by a specific commit. Takes exactly one file: a directory path is an error, call gitStatus first to find out which files changed. Path is relative to the current access-mode root (documentation root in Docs-only mode, repository root in Full-repo mode). Use this to reason about what changed recently, not just the current file content. Combine with readFile to understand both current state and history. Returns a unified diff (truncated for large changes) plus +/- line counts."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to the current access-mode root."
                },
                "scope": {
                    "type": ["string", "null"],
                    "enum": ["unstaged", "staged", null],
                    "description": "Working-tree scope: \"unstaged\" (default) or \"staged\". Ignored when `commit` is set."
                },
                "commit": {
                    "type": ["string", "null"],
                    "description": "Optional commit hash/ref. When set, returns the parent→commit file diff and ignores `scope`."
                }
            },
            "required": ["path"]
        }),
        }
}

/// The `gitStatus` schema the model sees.
pub(super) fn status_definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "gitStatus".to_string(),
        description:
            "List every file with uncommitted changes right now — staged, unstaged/untracked, and conflicted — plus the current branch. Takes no arguments. Paths come back relative to the current access-mode root, and only cover it (in Docs-only mode, changes outside the documentation root are not listed). This is the entry point for any question about \"what changed\" / \"what is uncommitted\": call it first, then gitDiff on the individual files it names — gitDiff itself only takes one file and cannot tell you which ones to ask about. Large working trees are capped and flagged `truncated`."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
        }
}

/// The `gitBlame` schema the model sees.
pub(super) fn blame_definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "gitBlame".to_string(),
        description:
            "Show line authorship (git blame) for one file as contiguous hunks sharing the same commit — who last changed which lines, when, and the commit summary. Path is relative to the current access-mode root. Optionally restrict to a 1-indexed inclusive line range; large ranges are capped. Use this to understand the history behind specific lines, not just their current content — investigate when a particular piece of content was introduced, or trace the origin of a decision or implementation detail. Combine with readFile to understand both current state and history."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to the current access-mode root."
                },
                "startLine": {
                    "type": ["integer", "null"],
                    "minimum": 1,
                    "description": "1-indexed first line (inclusive). Omit or null to start from line 1."
                },
                "endLine": {
                    "type": ["integer", "null"],
                    "minimum": 1,
                    "description": "1-indexed last line (inclusive). Omit or null to continue through the file (still subject to the per-call line cap)."
                }
            },
            "required": ["path"]
        }),
        }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use crate::domain::ai_access::AiAccessMode;
    use crate::domain::ai_tools::{
    GitBlameArgs, GitDiffArgs, ToolCall, ToolError, ToolResult, ToolScope,
};
    use crate::services::ai_tools::testing::*;
    use crate::services::ai_tools::{EmbeddingDeps, execute_tool};

    #[test]
    fn git_blame_on_an_uncommitted_file_explains_there_is_no_history_yet() {
        // The libgit2 text is quoted verbatim from a real run: blaming a
        // file whose whole folder had never been committed.
        let err = super::blame_error(
            "_benchmark_tools/x.adoc",
            crate::domain::git::GitError::Operation(
                "the path '_benchmark_tools' does not exist in the given tree; class=Tree (14); code=NotFound (-3)"
                    .to_string(),
            ),
        );
        let text = err.to_string();
        assert!(!text.contains("does not exist in the given tree"), "got: {text}");
        assert!(text.contains("не закоммичен"), "got: {text}");
        assert!(text.contains("gitDiff"), "should point at what does work: {text}");
        assert!(text.contains("_benchmark_tools/x.adoc"), "should name the file: {text}");
    }

    #[test]
    fn an_unrelated_git_failure_is_passed_through_untouched() {
        let err = super::blame_error(
            "a.adoc",
            crate::domain::git::GitError::NotARepository("/tmp/nope".to_string()),
        );
        assert!(err.to_string().contains("not a git repository"), "got: {err}");
    }

    #[test]
    fn git_diff_and_git_blame_reject_paths_outside_docs_root_in_docs_only() {
        let (repo, docs) = fixture_repo();
        // Real git repo so the tools get past open_repo — containment must
        // still fail before any blob read.
        {
            let git_repo = git2::Repository::init(&repo).unwrap();
            let mut config = git_repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps::empty();

        let err = execute_tool(
            &scope,
            ToolCall::GitDiff(GitDiffArgs {
                path: "../src/main.rs".to_string(),
                scope: None,
                commit: None,
            }),
            &deps,
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)), "got {err:?}");

        let err = execute_tool(
            &scope,
            ToolCall::GitBlame(GitBlameArgs {
                path: "../src/main.rs".to_string(),
                start_line: None,
                end_line: None,
            }),
            &deps,
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)), "got {err:?}");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn git_status_lists_changed_files_and_hides_those_outside_the_docs_root() {
        let (repo, docs) = fixture_repo();
        {
            let git_repo = git2::Repository::init(&repo).unwrap();
            let mut config = git_repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        // Untracked in both subtrees; only the docs one is in scope.
        fs::write(docs.join("new.adoc"), "= New\n").unwrap();
        fs::write(repo.join("src").join("main.rs"), "fn main() {}\n").unwrap();

        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let result = execute_tool(&scope, ToolCall::GitStatus, &EmbeddingDeps::empty(), &[]).unwrap();
        match result {
            ToolResult::GitStatus { staged, unstaged, conflicted, truncated, .. } => {
                let paths: Vec<&str> = unstaged.iter().map(|f| f.path.as_str()).collect();
                assert!(paths.contains(&"new.adoc"), "docs change, rebased: {paths:?}");
                assert!(
                    !paths.iter().any(|p| p.contains("main.rs")),
                    "src/ is outside Docs-only scope: {paths:?}"
                );
                assert!(staged.is_empty() && conflicted.is_empty());
                assert!(!truncated);
            }
            other => panic!("expected GitStatus, got {other:?}"),
        }

        // Full-repo sees both, with repo-relative paths.
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let result = execute_tool(&scope, ToolCall::GitStatus, &EmbeddingDeps::empty(), &[]).unwrap();
        match result {
            ToolResult::GitStatus { unstaged, .. } => {
                let paths: Vec<&str> = unstaged.iter().map(|f| f.path.as_str()).collect();
                assert!(paths.contains(&"docs/new.adoc"), "{paths:?}");
                assert!(paths.contains(&"src/main.rs"), "{paths:?}");
            }
            other => panic!("expected GitStatus, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn git_diff_on_a_directory_errors_instead_of_reporting_an_empty_diff() {
        let (repo, docs) = fixture_repo();
        {
            let git_repo = git2::Repository::init(&repo).unwrap();
            let mut config = git_repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        // Full-repo mode: `.` resolves to the repo root itself, which used to
        // reach git2's Index::get_path and panic the whole worker.
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let deps = EmbeddingDeps::empty();

        for path in [".", "docs"] {
            let err = execute_tool(
                &scope,
                ToolCall::GitDiff(GitDiffArgs {
                    path: path.to_string(),
                    scope: None,
                    commit: None,
                }),
                &deps,
                &[],
            )
            .unwrap_err();
            assert!(
                matches!(err, ToolError::InvalidArguments { .. }),
                "{path}: got {err:?}"
            );
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn git_diff_returns_unified_diff_for_unstaged_change_under_docs_root() {
        let (repo, docs) = fixture_repo();
        {
            let git_repo = git2::Repository::init(&repo).unwrap();
            let mut config = git_repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
            // Commit the docs file, then dirty the worktree.
            let mut index = git_repo.index().unwrap();
            index.add_path(Path::new("docs/intro.adoc")).unwrap();
            index.write().unwrap();
            let tree_oid = index.write_tree().unwrap();
            let tree = git_repo.find_tree(tree_oid).unwrap();
            let sig = git2::Signature::now("Test", "test@test.com").unwrap();
            git_repo
                .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap();
        }
        fs::write(docs.join("intro.adoc"), "= Intro\nchanged\n").unwrap();

        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let result = execute_tool(
            &scope,
            ToolCall::GitDiff(GitDiffArgs {
                path: "intro.adoc".to_string(),
                scope: Some("unstaged".to_string()),
                commit: None,
            }),
            &EmbeddingDeps::empty(),
            &[],
        )
        .unwrap();
        match result {
            ToolResult::GitDiff { path, label, diff, is_binary } => {
                assert_eq!(path, "intro.adoc");
                assert!(label.contains("Working tree") || label.contains("Index") || label.contains("HEAD"));
                assert!(!is_binary);
                assert!(diff.lines_added > 0 || diff.unified_diff.contains('+'));
            }
            other => panic!("expected GitDiff, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }
}
