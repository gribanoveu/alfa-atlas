//! `writeFile` — whole-file writes, always under the documentation root
//! even in Full-repo mode.

use crate::domain::ai_tools::{FileDiffStats, ToolError, ToolScope, WriteFileArgs};
use crate::domain::asciidoc_macro_brackets::ensure_macro_attribute_brackets;
use crate::domain::llm::LlmToolDefinition;
use crate::domain::project_config::ProjectError;
use crate::domain::supported_files;
use crate::services::{docs_fs, text_diff};

use super::super::EmbeddingDeps;
use super::super::resolve::{reject_atlas_memory_path, resolve_mutable_docs_path};

/// Append `[]` to bare `include::`/`image::`/`xref:` targets in AsciiDoc
/// files the assistant writes, so a missing attribute list does not make
/// the macro invisible to the index and diagnostics.
pub(super) fn close_adoc_macro_brackets(docs_rel: &str, content: String) -> String {
    if supported_files::is_asciidoc(docs_rel) {
        ensure_macro_attribute_brackets(&content)
    } else {
        content
    }
}

/// Resolves the path against the access-mode root, then requires it under
/// `docs_root` — Full-repo widens what the assistant can *read*, not what
/// it may write. Reuses `docs_fs::write_project_file` with the docs-relative
/// path: create-or-overwrite, creates parent directories, rejects unsupported
/// extensions.
pub(super) fn write_file(
    scope: &ToolScope,
    args: WriteFileArgs,
    deps: &EmbeddingDeps,
) -> Result<(String, FileDiffStats), ToolError> {
    let (access_rel, docs_rel) = resolve_mutable_docs_path(scope, &args.path)?;
    reject_atlas_memory_path(scope, &docs_rel)?;
    let docs_root = scope.docs_root.to_string_lossy();
    // NotFound-tolerant read, same pattern as `commands::project::
    // read_project_file_or_none` — a brand-new file diffs against `""`
    // rather than needing its own special case.
    let old = match docs_fs::read_project_file(&docs_root, &docs_rel) {
        Ok(content) => content,
        Err(ProjectError::NotFound(_)) => String::new(),
        Err(e) => return Err(e.into()),
    };
    let content = close_adoc_macro_brackets(&docs_rel, args.content);
    docs_fs::write_project_file(&docs_root, &docs_rel, &content)?;
    // Best-effort: keeps the in-memory index in step with this write
    // immediately, rather than only once the async file-watcher gets to it
    // — which otherwise regularly still lags behind by the time the next
    // tool call in the same round (e.g. `move`'s reference lookup, or
    // `check`) reads the index. Never fails the call itself — a write that
    // succeeded on disk must not be reported as failed just because the
    // index update lagged (e.g. `EmbeddingDeps::empty()` in tests, or no
    // project open).
    let _ = deps.workspace_index.update_document(scope.docs_root.join(&docs_rel));
    let diff = text_diff::diff_stats(&old, &content);
    Ok((access_rel, diff))
}

/// The `writeFile` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "writeFile".to_string(),
        description:
            "Create or overwrite one documentation file's full content, given its path relative to the current access-mode root (same as readFile/listFiles). The path must resolve under the documentation tree — paths outside it are rejected with an error. Any missing parent directories in the path are created automatically — there is no need to call createDirectory first. Always requires explicit user approval before the write actually happens — the user may deny it, in which case the file is left unchanged. Do not retry automatically after a denial; ask the user how they'd like to proceed instead. Only recognized documentation file types can be written."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to the current access-mode root (documentation root in Docs-only, repository root in Full-repo — same as readFile). Must resolve under the documentation tree; paths outside it are rejected. Must be a recognized documentation file type."
                },
                "content": {
                    "type": "string",
                    "description": "The full new content of the file, replacing any existing content."
                }
            },
            "required": ["path", "content"]
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
    fn write_file_creates_and_overwrites_under_docs_root() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        write(&scope, "new.adoc", "= New\n").unwrap();
        assert_eq!(fs::read_to_string(docs.join("new.adoc")).unwrap(), "= New\n");

        write(&scope, "new.adoc", "= Replaced\n").unwrap();
        assert_eq!(fs::read_to_string(docs.join("new.adoc")).unwrap(), "= Replaced\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn write_file_closes_bare_asciidoc_macros() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        write(&scope, "inc.adoc", "include::request.adoc\n").unwrap();
        assert_eq!(
            fs::read_to_string(docs.join("inc.adoc")).unwrap(),
            "include::request.adoc[]\n"
        );

        fs::remove_dir_all(&repo).ok();
    }

    /// The load-bearing containment guarantee for `WriteFile`: even in
    /// `FullRepo` mode (where `ReadFile`/`ListFiles` resolve against
    /// `repo_root`), a write must land under `docs_root` — `FullRepo` widens
    /// read context, not write license.
    #[test]
    fn write_file_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        // Bare docs-relative path of a file that does not exist yet is still
        // outside docs — the alias only rewrites when the target is already
        // on disk under docs_root.
        let err = write(&full_repo, "guide.adoc", "= Nope\n").unwrap_err();
        assert!(matches!(err, ToolError::OutsideDocumentation(_)), "got {err:?}");
        assert!(!docs.join("guide.adoc").exists());

        let written = write(&full_repo, "docs/guide.adoc", "= Guide\n").unwrap();
        assert_eq!(written, "docs/guide.adoc");
        assert_eq!(fs::read_to_string(docs.join("guide.adoc")).unwrap(), "= Guide\n");
        assert!(!repo.join("guide.adoc").exists());

        // Same file, docs-relative spelling: now exists, so the alias applies.
        let written = write(&full_repo, "guide.adoc", "= Aliased\n").unwrap();
        assert_eq!(written, "docs/guide.adoc");
        assert_eq!(fs::read_to_string(docs.join("guide.adoc")).unwrap(), "= Aliased\n");

        let err = write(&full_repo, "src/main.rs", "fn main() {}\n").unwrap_err();
        assert!(matches!(err, ToolError::OutsideDocumentation(_)), "got {err:?}");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn write_file_rejects_unsupported_extension() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = write(&scope, "notes.py", "print('nope')\n").unwrap_err();
        assert!(matches!(err, ToolError::Io(_)));
        assert!(!docs.join("notes.py").exists());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn write_file_rejects_atlas_memory_store() {
        let (repo, _docs) = fixture_repo();
        // When docs root is the repo itself, `.txt` under `.atlas/memory`
        // would otherwise be a writable supported path.
        let scope = ToolScope::for_project(&repo, &repo, AiAccessMode::DocsOnly);
        let err = write(&scope, ".atlas/memory/LOG.txt", "hijack\n").unwrap_err();
        assert!(
            matches!(err, ToolError::PathEscape(ref p) if p.contains(".atlas/memory")),
            "got {err:?}"
        );
        assert!(!repo.join(".atlas/memory/LOG.txt").exists());
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn write_file_rejects_path_escape() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = write(&scope, "../outside.adoc", "= Leak\n").unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));
        assert!(!repo.join("outside.adoc").exists());

        fs::remove_dir_all(&repo).ok();
    }
}
