//! `createDirectory` — a new folder, optionally scaffolded from a
//! template (today only the REST-endpoint one).

use crate::domain::ai_tools::{CreateDirectoryArgs, ToolError, ToolScope};
use crate::domain::llm::LlmToolDefinition;
use crate::services::docs_fs;

use super::super::EmbeddingDeps;
use super::super::resolve::{
    basename, docs_rel_to_access_rel, reject_atlas_memory_path, resolve_mutable_docs_path,
};

/// Same docs-root containment as `write_file`. Without a template, reuses
/// `docs_fs::create_project_dir` (creates missing parents, fails if the path
/// already exists). With `template: "restEndpoint"`, reuses
/// `docs_fs::create_rest_endpoint_folder`. Returns `(access_path, template,
/// created_files)` with access-mode-relative paths.
pub(super) fn create_directory(
    scope: &ToolScope,
    args: CreateDirectoryArgs,
    deps: &EmbeddingDeps,
) -> Result<(String, Option<String>, Vec<String>), ToolError> {
    let (access_rel, docs_rel) = resolve_mutable_docs_path(scope, &args.path)?;
    reject_atlas_memory_path(scope, &docs_rel)?;
    let docs_root = scope.docs_root.to_string_lossy();
    match args.template.as_deref() {
        None => {
            docs_fs::create_project_dir(&docs_root, &docs_rel)?;
            Ok((access_rel, None, Vec::new()))
        }
        Some("restEndpoint") => {
            let method_name = basename(&docs_rel);
            if method_name.is_empty() || method_name == "." {
                return Err(ToolError::InvalidArguments {
                    tool: "createDirectory".into(),
                    reason: "path must end with a folder name to use as the REST method name"
                        .into(),
                });
            }
            let created_docs = rest_endpoint_created_files(&docs_rel, method_name);
            docs_fs::create_rest_endpoint_folder(&docs_root, &docs_rel, method_name)?;
            // See `write_file`'s matching comment — same best-effort sync,
            // for every file the scaffold just created.
            for file in &created_docs {
                let _ = deps.workspace_index.update_document(scope.docs_root.join(file));
            }
            let created_files: Vec<String> = created_docs
                .into_iter()
                .map(|p| docs_rel_to_access_rel(scope, &p))
                .collect();
            Ok((access_rel, Some("restEndpoint".into()), created_files))
        }
        Some(other) => Err(ToolError::InvalidArguments {
            tool: "createDirectory".into(),
            reason: format!(
                "unknown template \"{other}\" (expected \"restEndpoint\" or null)"
            ),
        }),
    }
}

/// Docs-relative paths `create_rest_endpoint_folder` writes for `method_name`
/// under `folder_path` — kept in sync with `docs_fs::create_rest_endpoint_folder`.
pub(super) fn rest_endpoint_created_files(folder_path: &str, method_name: &str) -> Vec<String> {
    let child = |name: &str| -> String {
        if folder_path.is_empty() || folder_path == "." {
            name.to_string()
        } else {
            format!("{folder_path}/{name}")
        }
    };
    vec![
        child(&format!("{method_name}.adoc")),
        child("request.adoc"),
        child("response.adoc"),
        child(&format!("{method_name}.puml")),
    ]
}

/// The `createDirectory` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "createDirectory".to_string(),
        description:
            "Create a directory (including any missing parent directories) given its path relative to the current access-mode root (same as readFile/listFiles). The path must resolve under the documentation tree — paths outside it are rejected with an error. Use this only when the directory itself must exist (for example, an empty folder or a template scaffold). Do not call it solely to prepare parent directories for writeFile — writeFile creates missing parent directories automatically. For a new REST API method's documentation folder, pass template: \"restEndpoint\" — the same scaffold the editor's \"New folder\" dialog offers: `{methodName}.adoc`, `request.adoc`, `response.adoc`, and `{methodName}.puml`, where `{methodName}` is the final path segment. The `request.adoc`/`response.adoc` names are always bare, never prefixed with the method name — one folder is one method by convention, so the prefix would be redundant; do not rename them to match differently-named legacy folders. Omit template (or pass null) for an empty directory. The call is subject to the project's approval settings; if approval is required and the user denies it, nothing is created. Fails if the path already exists as a file or directory, so a successful result always means the folder is newly created, never pre-existing. On success, the result's `createdFiles` field already lists every generated file's exact path — treat it as authoritative and do not call listFiles to re-verify what was created."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path relative to the current access-mode root (same as readFile). Must resolve under the documentation tree. For template: \"restEndpoint\", the final path segment is used as the method name for generated filenames."
                },
                "template": {
                    "type": ["string", "null"],
                    "enum": ["restEndpoint", null],
                    "description": "Optional folder scaffold. \"restEndpoint\" creates REST-method documentation files inside the new folder (same as the UI template \"Документация на REST метод\"). Omit or null for an empty directory."
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
    fn create_directory_creates_missing_parents_under_docs_root() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        create_dir(&scope, "guides/nested").unwrap();

        assert!(docs.join("guides/nested").is_dir());

        fs::remove_dir_all(&repo).ok();
    }

    /// Same containment guarantee as `write_file_full_repo_mode_still_targets_docs_root_not_repo_root`:
    /// even in `FullRepo` mode a new directory must land under `docs_root`.
    #[test]
    fn create_directory_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        create_dir(&full_repo, "docs/endpoints").unwrap();

        assert!(docs.join("endpoints").is_dir());
        assert!(!repo.join("endpoints").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn create_directory_rejects_an_already_existing_path() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        create_dir(&scope, "guides").unwrap();
        let err = create_dir(&scope, "guides").unwrap_err();
        // `ProjectError::AlreadyExists` gained an explicit `ToolError`
        // mapping once `Move` needed to distinguish it from a generic IO
        // failure — `CreateDirectory` picks up the same precision as a
        // side effect, no longer the generic `Io` catch-all.
        assert!(matches!(err, ToolError::AlreadyExists(_)));

        // Also rejected when the path is already occupied by a file.
        let err = create_dir(&scope, "intro.adoc").unwrap_err();
        assert!(matches!(err, ToolError::AlreadyExists(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn create_directory_rejects_path_escape() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // `reject_atlas_memory_path` runs before `create_project_dir` and
        // itself calls `paths::join_relative` on the raw arg, which rejects
        // a `..` component with `ProjectError::PathEscape` — mapped
        // directly to `ToolError::PathEscape` by `From<ProjectError> for
        // ToolError`, so `create_project_dir`'s own
        // `validate_relative_name`/`InvalidName` path is never reached.
        let err = create_dir(&scope, "../outside-dir").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));
        assert!(!repo.join("outside-dir").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn create_directory_rest_endpoint_template_creates_scaffold_files() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let (path, template, created_files) =
            create_dir_with_template(&scope, "api/getUser", Some("restEndpoint")).unwrap();

        assert_eq!(path, "api/getUser");
        assert_eq!(template.as_deref(), Some("restEndpoint"));
        assert_eq!(
            created_files,
            vec![
                "api/getUser/getUser.adoc",
                "api/getUser/request.adoc",
                "api/getUser/response.adoc",
                "api/getUser/getUser.puml",
            ]
        );
        for rel in &created_files {
            assert!(docs.join(rel).is_file(), "missing scaffold file {rel}");
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn create_directory_rejects_unknown_template() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = create_dir_with_template(&scope, "api/foo", Some("openapi")).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { .. }));
        assert!(!docs.join("api/foo").exists());

        fs::remove_dir_all(&repo).ok();
    }
}
