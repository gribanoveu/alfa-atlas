//! `move` — rename or relocate, rewriting every reference to the moved
//! path in the rest of the docs. The reference rewriting is shared with
//! the UI's own rename (`services::reference_rewrite`), so both surfaces
//! leave the same links behind.

use crate::domain::ai_tools::{MoveArgs, ToolError, ToolScope};
use crate::domain::llm::LlmToolDefinition;
use crate::domain::project_config::UpdatedReference;
use crate::services::{docs_fs, reference_rewrite};

use super::super::EmbeddingDeps;
use super::super::resolve::{
    docs_rel_to_access_rel, reject_atlas_memory_path, resolve_mutable_docs_path,
};

/// Covers both moving and renaming, both files and directories — path args
/// use the access-mode root, then must resolve under `docs_root`. Picks
/// `docs_fs::rename_project_file` vs `rename_project_dir` via a cheap,
/// non-canonicalized `is_dir()` probe. Returns `(from, to, updated_files)`
/// with access-mode-relative paths (including `updated_files` entries).
pub(super) fn move_path(
    scope: &ToolScope,
    args: MoveArgs,
    deps: &EmbeddingDeps,
) -> Result<(String, String, Vec<UpdatedReference>), ToolError> {
    let (access_from, docs_from) = resolve_mutable_docs_path(scope, &args.path)?;
    let (access_to, docs_to) = resolve_mutable_docs_path(scope, &args.new_path)?;
    reject_atlas_memory_path(scope, &docs_from)?;
    reject_atlas_memory_path(scope, &docs_to)?;
    let docs_root = scope.docs_root.to_string_lossy();
    let is_dir = scope.docs_root.join(&docs_from).is_dir();

    // Also captures `renamed` (the repo-relative old/new pairs) so it's
    // still available below, after the actual filesystem move, to update
    // the moved document(s)' own index rows — empty when `docs_root`
    // doesn't resolve under `repo_root`, same "skip the cascade entirely"
    // case `updated_files` already handles.
    let mut renamed: Vec<reference_rewrite::RenamedPath> = Vec::new();
    let mut updated_files =
        match reference_rewrite::docs_root_suffix(&scope.repo_root, &docs_root) {
            Some(suffix) => {
                let old = reference_rewrite::to_repo_relative(&suffix, &docs_from);
                let new = reference_rewrite::to_repo_relative(&suffix, &docs_to);
                renamed = if is_dir {
                    reference_rewrite::renamed_paths_for_dir_move(&deps.workspace_index, &old, &new)
                } else {
                    vec![reference_rewrite::RenamedPath { old, new }]
                };
                let rewritten = reference_rewrite::rewrite_references(
                    &deps.workspace_index,
                    &scope.repo_root,
                    &renamed,
                )
                .map_err(|e| ToolError::Io(std::io::Error::other(e.to_string())))?;
                reference_rewrite::into_report(&suffix, rewritten).updated_files
            }
            None => Vec::new(),
        };

    if is_dir {
        docs_fs::rename_project_dir(&docs_root, &docs_from, &docs_to)?;
    } else {
        docs_fs::rename_project_file(&docs_root, &docs_from, &docs_to)?;
    }

    // See `write_file`'s matching comment — same best-effort sync, so the
    // moved document(s) themselves are found at their new path immediately
    // (their content still gets re-parsed asynchronously, same as any
    // other `update_document`/`index_file` call, but their existence and
    // path are correct right away — including for a `move` chained
    // straight after this one in the same round).
    for pair in &renamed {
        let _ = deps
            .workspace_index
            .rename_document(scope.repo_root.join(&pair.old), scope.repo_root.join(&pair.new));
    }

    for u in &mut updated_files {
        u.docs_relative_path = docs_rel_to_access_rel(scope, &u.docs_relative_path);
    }

    Ok((access_from, access_to, updated_files))
}

/// The `move` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "move".to_string(),
        description:
            "Move or rename a file or directory, given its current path and a new path, both relative to the current access-mode root (same as readFile). Both must resolve under the documentation tree — paths outside it are rejected with an error. This is one operation covering both cases: a newPath in the same directory with a different name is a rename, a newPath elsewhere is a move (optionally with a new name too). Works for both files and directories — there is no separate rename tool or directory-specific variant. References to the old path elsewhere in the documentation (include::, xref:, and JSON/YAML $ref) are updated automatically so they keep pointing at the right file. Fails if something already exists at newPath — nothing is overwritten. newPath's parent directory must already exist — use createDirectory first if it doesn't. Unlike writeFile, move does not create missing parent directories. Always requires explicit user approval before anything changes — the user may deny it."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Current path relative to the current access-mode root (same as readFile). Must resolve under the documentation tree."
                },
                "newPath": {
                    "type": "string",
                    "description": "New path relative to the current access-mode root (same as readFile). Must resolve under the documentation tree. Fails if something already exists there."
                }
            },
            "required": ["path", "newPath"]
        }),
        }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::domain::ai_access::AiAccessMode;
    use crate::domain::ai_tools::{ToolCall, ToolError, ToolScope, WriteFileArgs};
    use crate::services::ai_tools::testing::*;
    use crate::services::ai_tools::{EmbeddingDeps, execute_tool};

    use super::*;

    #[test]
    fn move_renames_a_file_in_place() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let (from, to, updated_files) = move_it(&scope, "intro.adoc", "introduction.adoc").unwrap();
        assert_eq!(from, "intro.adoc");
        assert_eq!(to, "introduction.adoc");
        assert!(updated_files.is_empty());
        assert!(!docs.join("intro.adoc").exists());
        assert!(docs.join("introduction.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_moves_a_file_to_a_different_directory() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("archive")).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        move_it(&scope, "intro.adoc", "archive/intro.adoc").unwrap();
        assert!(!docs.join("intro.adoc").exists());
        assert!(docs.join("archive/intro.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_moves_and_renames_a_directory_together() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("old/nested")).unwrap();
        fs::write(docs.join("old/nested/page.adoc"), "= Page\n").unwrap();
        // `rename_project_dir`/`fs::rename` need the destination's parent to
        // already exist — unlike `write_project_file`, neither
        // `rename_project_file` nor `rename_project_dir` auto-creates it
        // (the manual drag-and-drop UI never hits this: a drop target is
        // always an existing folder).
        fs::create_dir_all(docs.join("archive")).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        move_it(&scope, "old", "archive/renamed").unwrap();
        assert!(!docs.join("old").exists());
        assert_eq!(
            fs::read_to_string(docs.join("archive/renamed/nested/page.adoc")).unwrap(),
            "= Page\n"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_rejects_when_destination_already_exists() {
        let (repo, docs) = fixture_repo();
        // `script.py` (fixture_repo's other file) is an unsupported
        // extension, which would fail with `UnsupportedFile` before ever
        // reaching the exists check — use another `.adoc` so this test
        // actually exercises `AlreadyExists`.
        fs::write(docs.join("other.adoc"), "= Other\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = move_it(&scope, "intro.adoc", "other.adoc").unwrap_err();
        assert!(matches!(err, ToolError::AlreadyExists(_)));
        // Nothing moved.
        assert!(docs.join("intro.adoc").exists());
        assert_eq!(fs::read_to_string(docs.join("other.adoc")).unwrap(), "= Other\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_rejects_missing_source() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = move_it(&scope, "does-not-exist.adoc", "new-name.adoc").unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        move_it(&full_repo, "docs/intro.adoc", "docs/renamed.adoc").unwrap();
        assert!(docs.join("renamed.adoc").exists());
        assert!(!repo.join("renamed.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_rejects_path_escape() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // `reject_atlas_memory_path` runs before `move_path`'s own path
        // handling and itself calls `paths::join_relative` on the raw arg,
        // which rejects a `..` component with `ProjectError::PathEscape` —
        // mapped directly to `ToolError::PathEscape` (see the matching
        // `create_directory_rejects_path_escape` comment).
        let err = move_it(&scope, "../outside.adoc", "new-name.adoc").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    /// The one test that wires up a real, built `WorkspaceIndex` instead of
    /// `EmbeddingDeps::empty()`'s blank one — proves `move_path` actually
    /// calls through to `reference_rewrite::rewrite_references` and reports
    /// the result, not just performs a bare `fs::rename`.
    #[test]
    fn move_rewrites_references_in_other_files() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("sub")).unwrap();
        fs::write(docs.join("sub/detail.adoc"), "= Detail\n").unwrap();
        fs::write(docs.join("guide.adoc"), "= Guide\n\ninclude::sub/detail.adoc[]\n").unwrap();
        // Same as `move_moves_and_renames_a_directory_together`: the
        // destination's parent must already exist.
        fs::create_dir_all(docs.join("sub2")).unwrap();

        let mut deps = EmbeddingDeps::empty();
        deps.workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let (from, to, updated_files) =
            move_it_with_deps(&scope, "sub/detail.adoc", "sub2/detail.adoc", &deps).unwrap();
        assert_eq!(from, "sub/detail.adoc");
        assert_eq!(to, "sub2/detail.adoc");
        assert_eq!(
            updated_files,
            vec![UpdatedReference { docs_relative_path: "guide.adoc".to_string(), count: 1 }]
        );
        assert!(!docs.join("sub/detail.adoc").exists());
        assert!(docs.join("sub2/detail.adoc").exists());
        assert_eq!(
            fs::read_to_string(docs.join("guide.adoc")).unwrap(),
            "= Guide\n\ninclude::sub2/detail.adoc[]\n"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_of_an_unreferenced_file_reports_zero_updated_references() {
        let (repo, docs) = fixture_repo();
        let mut deps = EmbeddingDeps::empty();
        deps.workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let (_, _, updated_files) =
            move_it_with_deps(&scope, "intro.adoc", "introduction.adoc", &deps).unwrap();
        assert!(updated_files.is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    /// Reproduces the exact gap a benchmark run of the file tools surfaced:
    /// a `writeFile` call creates a file that references another, then a
    /// `move` call in the same turn relocates the referenced file — before
    /// `write_file` synchronously called `update_document`, `move`'s
    /// reference lookup ran against a `WorkspaceIndex` that had never heard
    /// of the just-written file, so it silently reported zero updated
    /// references even though the include was right there on disk.
    #[test]
    fn move_finds_a_reference_written_by_write_file_earlier_in_the_same_turn() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("sub")).unwrap();
        fs::write(docs.join("sub/detail.adoc"), "= Detail\n").unwrap();
        fs::create_dir_all(docs.join("sub2")).unwrap();

        let mut deps = EmbeddingDeps::empty();
        deps.workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // `guide.adoc` doesn't exist yet when the index above is built —
        // it's created through the real tool, same as the AI would.
        execute_tool(
            &scope,
            ToolCall::WriteFile(WriteFileArgs {
                path: "guide.adoc".to_string(),
                content: "= Guide\n\ninclude::sub/detail.adoc[]\n".to_string(),
            }),
            &deps,
            &[],
        )
        .unwrap();

        let (_, _, updated_files) =
            move_it_with_deps(&scope, "sub/detail.adoc", "sub2/detail.adoc", &deps).unwrap();
        assert_eq!(
            updated_files,
            vec![UpdatedReference { docs_relative_path: "guide.adoc".to_string(), count: 1 }]
        );
        assert_eq!(
            fs::read_to_string(docs.join("guide.adoc")).unwrap(),
            "= Guide\n\ninclude::sub2/detail.adoc[]\n"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn move_updates_the_moved_documents_own_row_in_the_index() {
        let (repo, docs) = fixture_repo();
        let mut deps = EmbeddingDeps::empty();
        deps.workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        move_it_with_deps(&scope, "intro.adoc", "introduction.adoc", &deps).unwrap();

        assert!(deps.workspace_index.get_document(&docs.join("intro.adoc")).is_none());
        assert!(deps.workspace_index.get_document(&docs.join("introduction.adoc")).is_some());

        fs::remove_dir_all(&repo).ok();
    }
}
