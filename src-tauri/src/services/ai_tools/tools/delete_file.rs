//! `deleteFile` — removal, scoped to the documentation root.

use crate::domain::ai_tools::{DeleteFileArgs, FileDiffStats, ToolError, ToolScope};
use crate::domain::llm::LlmToolDefinition;
use crate::services::{docs_fs, text_diff};

use super::super::EmbeddingDeps;
use super::super::resolve::{reject_atlas_memory_path, resolve_mutable_docs_path};

/// Same docs-root containment as `write_file`. Reuses
/// `docs_fs::delete_project_file`: fails if the path is missing or not a file.
pub(super) fn delete_file(
    scope: &ToolScope,
    args: DeleteFileArgs,
    deps: &EmbeddingDeps,
) -> Result<(String, FileDiffStats), ToolError> {
    let (access_rel, docs_rel) = resolve_mutable_docs_path(scope, &args.path)?;
    reject_atlas_memory_path(scope, &docs_rel)?;
    let docs_root = scope.docs_root.to_string_lossy();
    // Missing-file already errors below via `delete_project_file`'s own
    // `NotFound` (see `delete_file_rejects_missing_file`) — this read just
    // surfaces that same error slightly earlier, with no fallback needed.
    let old = docs_fs::read_project_file(&docs_root, &docs_rel)?;
    docs_fs::delete_project_file(&docs_root, &docs_rel)?;
    // See `write_file`'s matching comment — same best-effort sync, so a
    // `check` (or another tool) called right after doesn't still see
    // diagnostics for a file that's already gone.
    let _ = deps.workspace_index.remove_document(scope.docs_root.join(&docs_rel));
    let diff = text_diff::diff_stats(&old, "");
    Ok((access_rel, diff))
}

/// The `deleteFile` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "deleteFile".to_string(),
        description:
            "Delete one file, given its path relative to the current access-mode root (same as readFile/listFiles). The path must resolve under the documentation tree — paths outside it are rejected with an error. This is irreversible — do not call it speculatively. Always requires explicit user approval before the deletion actually happens — the user may deny it, in which case the file is left unchanged."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to the current access-mode root (same as readFile). Must resolve under the documentation tree; paths outside it are rejected."
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
    use crate::domain::ai_tools::{ToolError, ToolScope};
    use crate::services::ai_tools::testing::*;

    #[test]
    fn delete_file_removes_the_file() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        delete(&scope, "intro.adoc").unwrap();
        assert!(!docs.join("intro.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_file_rejects_missing_file() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = delete(&scope, "does-not-exist.adoc").unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_file_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        delete(&full_repo, "docs/intro.adoc").unwrap();
        assert!(!docs.join("intro.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_file_rejects_path_escape() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // `delete_file` now reads the file first (to diff against on
        // deletion — see `text_diff::diff_stats`), and `read_project_file`
        // validates via `join_relative` rather than `validate_relative_name`
        // — so `..` surfaces as `PathEscape` here rather than the
        // `InvalidName`-via-`Io` catch-all `delete_project_file` alone would
        // have produced. Equally rejected either way; this is just the more
        // specific variant.
        let err = delete(&scope, "../outside.adoc").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }
}
