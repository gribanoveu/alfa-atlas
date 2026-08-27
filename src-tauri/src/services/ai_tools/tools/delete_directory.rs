//! `deleteDirectory` — removal of a folder, non-recursive unless the
//! model asks, so an over-broad path costs one refusal rather than a tree.

use crate::domain::ai_tools::{DeleteDirectoryArgs, ToolError, ToolScope};
use crate::domain::llm::LlmToolDefinition;
use crate::services::docs_fs;

use super::super::EmbeddingDeps;
use super::super::resolve::{reject_atlas_memory_path, resolve_mutable_docs_path};

/// Same docs-root containment as `write_file`.
/// `recursive` defaults to `false` when omitted (`Option::unwrap_or`) —
/// `docs_fs::delete_project_dir` then refuses a non-empty directory with
/// `ToolError::DirectoryNotEmpty` rather than silently deleting its
/// contents; pass `recursive: true` to delete a non-empty directory in one
/// call.
pub(super) fn delete_directory(
    scope: &ToolScope,
    args: DeleteDirectoryArgs,
    deps: &EmbeddingDeps,
) -> Result<String, ToolError> {
    let (access_rel, docs_rel) = resolve_mutable_docs_path(scope, &args.path)?;
    reject_atlas_memory_path(scope, &docs_rel)?;
    let recursive = args.recursive.unwrap_or(false);
    // Only a `recursive` delete can remove indexed files at all — a
    // non-recursive delete only ever succeeds against an already-empty
    // directory, which by definition holds nothing the index tracks.
    // Enumerated and cleared from the index *before* the actual removal
    // (mirrors `move_path`'s "read the index before mutating the
    // filesystem" ordering): `remove_document` resolves each file's index
    // id by walking up from its path, which breaks once a parent
    // directory is gone — as every parent below the top one would be,
    // partway through removing a multi-level subtree.
    if recursive {
        let abs_dir = scope.docs_root.join(&docs_rel);
        if let Ok(nested) = crate::infra::workspace_scanner::scan_all(&abs_dir) {
            for file in nested {
                let _ = deps.workspace_index.remove_document(file.path);
            }
        }
    }
    docs_fs::delete_project_dir(&scope.docs_root.to_string_lossy(), &docs_rel, recursive)?;
    Ok(access_rel)
}

/// The `deleteDirectory` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "deleteDirectory".to_string(),
        description:
            "Delete a directory, given its path relative to the current access-mode root (same as readFile/listFiles). The path must resolve under the documentation tree — paths outside it are rejected with an error. By default (recursive omitted or false), the call is rejected if the directory is not empty — delete its contents first, or pass recursive: true to delete the directory and everything inside it in one call. This is irreversible, especially with recursive: true — do not call it speculatively. Always requires explicit user approval before the deletion actually happens — the user may deny it."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path relative to the current access-mode root (same as readFile). Must resolve under the documentation tree; paths outside it are rejected."
                },
                "recursive": {
                    "type": ["boolean", "null"],
                    "description": "If true, deletes the directory and all its contents. If omitted or false (default), the call is rejected when the directory is not empty."
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
    use crate::domain::ai_tools::{DeleteDirectoryArgs, ToolCall, ToolError, ToolScope};
    use crate::services::ai_tools::testing::*;
    use crate::services::ai_tools::{EmbeddingDeps, execute_tool};

    #[test]
    fn delete_directory_removes_an_empty_directory_by_default() {
        let (repo, docs) = fixture_repo();
        create_dir(&ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly), "empty").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        delete_dir(&scope, "empty", None).unwrap();
        assert!(!docs.join("empty").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_directory_refuses_a_non_empty_directory_by_default() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("folder")).unwrap();
        fs::write(docs.join("folder/note.adoc"), "= Note\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = delete_dir(&scope, "folder", None).unwrap_err();
        assert!(matches!(err, ToolError::DirectoryNotEmpty(_)));
        assert!(docs.join("folder/note.adoc").exists());

        // Explicit `recursive: false` behaves the same as omitting it.
        let err = delete_dir(&scope, "folder", Some(false)).unwrap_err();
        assert!(matches!(err, ToolError::DirectoryNotEmpty(_)));
        assert!(docs.join("folder/note.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_directory_recursive_true_deletes_a_non_empty_directory() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("folder")).unwrap();
        fs::write(docs.join("folder/note.adoc"), "= Note\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        delete_dir(&scope, "folder", Some(true)).unwrap();
        assert!(!docs.join("folder").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_directory_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("empty")).unwrap();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        delete_dir(&full_repo, "docs/empty", None).unwrap();
        assert!(!docs.join("empty").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn delete_directory_rejects_missing_directory() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = delete_dir(&scope, "does-not-exist", None).unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));

        fs::remove_dir_all(&repo).ok();
    }

    /// The other half of the benchmark-surfaced gap: a recursive
    /// `deleteDirectory` used to leave every nested file's row behind in
    /// the index (only the top-level fs subtree was ever touched — nothing
    /// in the AI tool path called `remove_document`), so a `check` right
    /// after still reported diagnostics for files that no longer existed
    /// on disk.
    #[test]
    fn delete_directory_recursive_removes_nested_documents_from_the_index() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("folder/nested")).unwrap();
        fs::write(docs.join("folder/note.adoc"), "= Note\n").unwrap();
        fs::write(docs.join("folder/nested/deep.adoc"), "= Deep\n").unwrap();

        let mut deps = EmbeddingDeps::empty();
        deps.workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        assert!(deps.workspace_index.get_document(&docs.join("folder/nested/deep.adoc")).is_some());

        execute_tool(
            &scope,
            ToolCall::DeleteDirectory(DeleteDirectoryArgs {
                path: "folder".to_string(),
                recursive: Some(true),
            }),
            &deps,
            &[],
        )
        .unwrap();

        assert!(deps.workspace_index.get_document(&docs.join("folder/note.adoc")).is_none());
        assert!(deps.workspace_index.get_document(&docs.join("folder/nested/deep.adoc")).is_none());

        fs::remove_dir_all(&repo).ok();
    }
}
