//! `check` — the two verifications the model can run against the docs it
//! just wrote: the diagnostics the editor itself shows (`problems`), and
//! the per-folder standards rules (`standards`).

use std::path::Path;

use crate::domain::ai_tools::{CheckArgs, CheckKind, ToolError, ToolResult, ToolScope};
use crate::domain::llm::LlmToolDefinition;
use crate::domain::paths;
use crate::domain::workspace_index::{Diagnostic, DocumentId, Table};
use crate::services::{diagnostics, reference_rewrite, standards, standards_prefs};

use super::super::EmbeddingDeps;
use super::super::resolve::{
    docs_rel_to_access_rel, resolve_mutable_docs_path, to_access_relative,
};

/// Cap on how many diagnostics a single `check` call may return — keeps the
/// tool-message payload bounded for a large docs tree.
pub(super) const MAX_CHECK_DIAGNOSTICS: usize = 200;

/// Cap on how many method-folder results a single `check` (`kind:
/// "standards"`) call may return — same rationale as
/// `MAX_CHECK_DIAGNOSTICS`. Failing folders are kept first (see
/// `check_doc_standards`), so a truncated response still surfaces the
/// folders most worth the model's attention.
pub(super) const MAX_STANDARDS_FOLDERS: usize = 100;

/// Recomputes workspace diagnostics then returns them — same findings as
/// BottomDock «Проблемы» / standards — path args use the access-mode root;
/// must still resolve under `docs_root`. Result paths are rewritten to
/// access-mode-relative before returning to the model.
pub(super) fn check(
    scope: &ToolScope,
    args: CheckArgs,
    deps: &EmbeddingDeps,
) -> Result<ToolResult, ToolError> {
    match args.kind {
        CheckKind::Problems => check_problems(scope, args.path.as_deref(), deps),
        CheckKind::Standards => check_doc_standards(scope, args.path.as_deref()),
    }
}

pub(super) fn check_problems(
    scope: &ToolScope,
    path: Option<&str>,
    deps: &EmbeddingDeps,
) -> Result<ToolResult, ToolError> {
    let mut diagnostics = match path {
        None => {
            diagnostics::run_all(&deps.workspace_index);
            deps.workspace_index.get_diagnostics()
        }
        Some(access_path) => {
            let doc_id = access_path_to_document_id(scope, access_path)?;
            // A `check` right after a `writeFile` is the common case, and
            // AsciiDoc facts arrive asynchronously from the frontend parser
            // (`workspace_index::submit_asciidoc_facts`). Without this wait
            // the answer describes the file as it was *before* the write —
            // the model's own edit, reported back as still broken or still
            // fine. Best-effort: a wait that times out still returns the
            // diagnostics we have, since one stale answer beats none.
            deps.workspace_index.wait_for_parse_settled(&doc_id);
            diagnostics::run_for(&deps.workspace_index, &doc_id);
            deps.workspace_index.get_diagnostics_for(&doc_id)
        }
    };

    let truncated = diagnostics.len() > MAX_CHECK_DIAGNOSTICS;
    if truncated {
        diagnostics.truncate(MAX_CHECK_DIAGNOSTICS);
    }

    for d in &mut diagnostics {
        if let Some(access) = to_access_relative(scope, &d.document.0) {
            let old = d.document.0.clone();
            if old != access {
                d.message = d.message.replace(&old, &access);
            }
            d.document = DocumentId::new(access);
        }
    }

    Ok(ToolResult::CheckResults {
        kind: CheckKind::Problems,
        diagnostics,
        truncated,
    })
}

/// Outcome of the automatic post-write check (`check_written_file`).
pub enum WriteCheck {
    /// The written document's own diagnostics (empty means clean), plus the
    /// shape asciidoctor resolved for each `|===` block in it.
    ///
    /// The two travel together because they answer the same question about
    /// the same write and are gated on the same wait: diagnostics say whether
    /// the file's references hold, tables say whether its markup produced the
    /// structure the author meant. Neither is derivable from the other.
    Settled { diagnostics: Vec<Diagnostic>, tables: Vec<Table> },
    /// The frontend parse did not come back within
    /// `WorkspaceIndex::wait_for_parse_settled`'s budget, so nothing is known
    /// about the file as it now stands. Distinct from `Settled(vec![])` on
    /// purpose — reporting "clean" here would be a guess.
    Unsettled,
}

/// The diagnostics for one just-written file, for `llm_chat` to append to
/// that write's tool result.
///
/// Deliberately the same three steps `check_problems` runs for a single
/// path — wait for the parse, recompute, read back — rather than a second,
/// cheaper approximation that could disagree with what an explicit `check`
/// would say about the very same file.
///
/// Scoped to the written document alone. `run_for` also refreshes the
/// documents that include or xref it, but their diagnostics are not read
/// here: most of them predate the write, and attributing them to it would
/// be worse than silence. Incoming breakage stays `check`'s job.
pub fn check_written_file(
    scope: &ToolScope,
    deps: &EmbeddingDeps,
    access_path: &str,
) -> Result<WriteCheck, ToolError> {
    let doc_id = access_path_to_document_id(scope, access_path)?;
    if !deps.workspace_index.wait_for_parse_settled(&doc_id) {
        return Ok(WriteCheck::Unsettled);
    }
    diagnostics::run_for(&deps.workspace_index, &doc_id);
    let mut diagnostics = deps.workspace_index.get_diagnostics_for(&doc_id);
    for d in &mut diagnostics {
        if let Some(access) = to_access_relative(scope, &d.document.0) {
            let old = d.document.0.clone();
            if old != access {
                d.message = d.message.replace(&old, &access);
            }
            d.document = DocumentId::new(access);
        }
    }
    let tables = deps.workspace_index.get_tables_for(&doc_id);
    Ok(WriteCheck::Settled { diagnostics, tables })
}

/// Access-mode-relative path → repo-relative `DocumentId`, after requiring
/// containment under `scope.docs_root`.
pub(super) fn access_path_to_document_id(scope: &ToolScope, access_path: &str) -> Result<DocumentId, ToolError> {
    let (_access_rel, docs_rel) = resolve_mutable_docs_path(scope, access_path)?;
    let docs_root = scope.docs_root.to_string_lossy();
    let suffix = reference_rewrite::docs_root_suffix(&scope.repo_root, &docs_root).ok_or_else(
        || ToolError::InvalidArguments {
            tool: "check".into(),
            reason: "documentation root is not under the repository root".into(),
        },
    )?;
    Ok(DocumentId::new(reference_rewrite::to_repo_relative(
        &suffix, &docs_rel,
    )))
}

/// Runs the API-documentation standards checker (`services::standards`,
/// same engine as the «Стандарты» panel's «Проверить» button) against the
/// whole docs root, then — when `path` narrows it — filters down to just
/// one method folder. Always walks the full docs root first rather than
/// rooting `check_repository` at the target folder directly: that
/// function's own root argument doubles as "the container, not itself a
/// checkable folder" (mirrors the real docs root never being a method
/// folder), so pointing it straight at a method folder would incorrectly
/// skip evaluating that very folder. This mirrors how the «Стандарты»
/// panel's "Текущий файл" tab scopes too (client-side filtering over a
/// full-project report). Unlike `check_problems`'s `path` (a single file),
/// `standards` checking operates at directory granularity — per К.1.1 of
/// the standard, a "unit" is a `methodName` folder, not a file — so a file
/// path is resolved to its parent directory rather than rejected.
pub(super) fn check_doc_standards(scope: &ToolScope, path: Option<&str>) -> Result<ToolResult, ToolError> {
    let config = standards_prefs::load_standards_config().unwrap_or_default();
    let mut report = standards::check_repository(&scope.docs_root, &config);

    if let Some(access_path) = path {
        let (_access_rel, docs_rel) = resolve_mutable_docs_path(scope, access_path)?;
        let joined = paths::join_relative(&scope.docs_root, &docs_rel)?;
        let canonical = paths::ensure_under(&scope.docs_root, &joined)?;
        let target_dir = if canonical.is_file() {
            canonical.parent().map(Path::to_path_buf).unwrap_or(canonical)
        } else {
            canonical
        };
        let target_rel =
            paths::relative_to(&scope.docs_root, &target_dir).unwrap_or_default();
        let prefix = format!("{target_rel}/");
        report.folders.retain(|f| f.folder == target_rel || f.folder.starts_with(&prefix));
        report.overall_passed =
            !report.folders.is_empty() && report.folders.iter().all(|f| f.passed);
    }

    // Failing folders first, so a truncated response still surfaces what's
    // most worth the model's attention.
    report.folders.sort_by_key(|f| f.passed);
    let truncated = report.folders.len() > MAX_STANDARDS_FOLDERS;
    if truncated {
        report.folders.truncate(MAX_STANDARDS_FOLDERS);
    }

    for folder in &mut report.folders {
        folder.folder = docs_rel_to_access_rel(scope, &folder.folder);
    }

    Ok(ToolResult::StandardsChecked { report, truncated })
}

/// The `check` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "check".to_string(),
        description:
            "Run a documentation verification and return findings. Two kinds are available:\n\nkind \"problems\": the same list the editor's Problems panel shows — broken xref/include/image targets, missing anchors, duplicate anchors, circular includes, and parse errors. Checks cover ONLY supported indexed documentation file types under the documentation root (.adoc/.asciidoc, .md/.markdown, .json, .yaml/.yml, .txt, .puml/.plantuml, .mmd/.mermaid) — not arbitrary repository source code or unsupported extensions. Recomputes diagnostics before returning so results are fresh. Use this to verify documentation integrity before and after edits.\n\nkind \"standards\": checks API-method documentation folders against the corporate documentation standard (К.1.1–К.7.1, weighted criteria, 80% pass threshold per method folder). Purely local file reads, no network access — link-correctness (К.1.3) is out of scope. Use this to audit API documentation quality.\n\nOptional `path` and every finding's `document` field use the current access-mode root (same as readFile/writeFile) — pass them between tools unchanged."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["problems", "standards"],
                    "description": "Which verification to run. \"problems\" = workspace diagnostics (Problems panel) for integrity checks. \"standards\" = API-documentation corporate standard compliance (К.1.1–К.7.1, weighted, 80% pass threshold per method folder)."
                },
                "path": {
                    "type": ["string", "null"],
                    "description": "Optional path relative to the current access-mode root (same as readFile); must resolve under the documentation tree. For \"problems\": a single file to check (omit/null to check all indexed documentation files). For \"standards\": a method folder to check, or any file inside one (its parent folder is used) — omit/null to check the entire documentation tree."
                }
            },
            "required": ["kind"]
        }),
        }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use crate::domain::ai_access::AiAccessMode;
    use crate::domain::ai_tools::{
    CheckArgs, CheckKind, ToolCall, ToolError, ToolResult, ToolScope,
};
    use crate::services::ai_tools::testing::*;
    use crate::services::ai_tools::{EmbeddingDeps, execute_tool};

    use super::*;

    #[test]
    fn check_problems_returns_broken_xref_for_all_and_for_one_file() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("broken.adoc"), "xref:nope.adoc[]\n").unwrap();
        fs::write(docs.join("clean.adoc"), "[[ok]]\n= Clean\n").unwrap();

        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps {
            workspace_index,
            ..EmbeddingDeps::empty()
        };

        let all = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: None,
            }),
            &deps,
            &[],
        )
        .unwrap();
        match all {
            ToolResult::CheckResults {
                kind,
                diagnostics,
                truncated,
            } => {
                assert_eq!(kind, CheckKind::Problems);
                assert!(!truncated);
                assert!(
                    diagnostics.iter().any(|d| {
                        d.kind
                            == crate::domain::workspace_index::DiagnosticKind::MissingXrefDocument
                            && d.document.as_str() == "broken.adoc"
                    }),
                    "got: {diagnostics:?}"
                );
                assert!(
                    !diagnostics
                        .iter()
                        .any(|d| d.document.as_str() == "clean.adoc"),
                    "clean file should not appear: {diagnostics:?}"
                );
            }
            other => panic!("expected CheckResults, got {other:?}"),
        }

        let one = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: Some("broken.adoc".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap();
        match one {
            ToolResult::CheckResults { diagnostics, .. } => {
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0].document.as_str(), "broken.adoc");
            }
            other => panic!("expected CheckResults, got {other:?}"),
        }

        let clean = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: Some("clean.adoc".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap();
        match clean {
            ToolResult::CheckResults { diagnostics, .. } => {
                assert!(diagnostics.is_empty(), "got: {diagnostics:?}");
            }
            other => panic!("expected CheckResults, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn check_problems_full_repo_returns_access_relative_document_paths() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("broken.adoc"), "xref:nope.adoc[]\n").unwrap();

        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let deps = EmbeddingDeps {
            workspace_index,
            ..EmbeddingDeps::empty()
        };

        let one = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: Some("docs/broken.adoc".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap();
        match one {
            ToolResult::CheckResults { diagnostics, .. } => {
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0].document.as_str(), "docs/broken.adoc");
            }
            other => panic!("expected CheckResults, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn check_problems_rejects_path_escape_under_docs_root() {
        let (repo, docs) = fixture_repo();
        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps {
            workspace_index,
            ..EmbeddingDeps::empty()
        };

        let err = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: Some("../src/main.rs".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)), "got {err:?}");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn check_problems_truncates_when_over_cap() {
        let (repo, docs) = fixture_repo();
        let mut body = String::new();
        for i in 0..(MAX_CHECK_DIAGNOSTICS + 5) {
            body.push_str(&format!("xref:missing{i}.adoc[]\n"));
        }
        fs::write(docs.join("many.adoc"), body).unwrap();

        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps {
            workspace_index,
            ..EmbeddingDeps::empty()
        };

        let result = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Problems,
                path: Some("many.adoc".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap();
        match result {
            ToolResult::CheckResults {
                diagnostics,
                truncated,
                ..
            } => {
                assert!(truncated);
                assert_eq!(diagnostics.len(), MAX_CHECK_DIAGNOSTICS);
            }
            other => panic!("expected CheckResults, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    /// Writes a `methodName` folder under `root` that passes every default
    /// standards rule (mirrors `services::standards::tests::write_full_method`).
    fn write_full_standards_method(root: &Path, method: &str) {
        let dir = root.join(method);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{method}.puml")), "@startuml\n@enduml").unwrap();
        let main = format!(
            "= {method}\n:toc:\n\n== Назначение\nЭто достаточно длинное описание метода для прохождения проверки на пятьдесят символов.\n\n== Схема работы\nimage::{method}.puml[]\n\n== Описание входных параметров\n|===\n| Имя | Тип | Обязательный | Описание\n| id | string | да | идентификатор\n|===\n\ninclude::./request.adoc[]\n\n== Описание выходных параметров\n|===\n| Имя | Тип | Обязательный | Описание\n| id | string | да | идентификатор\n|===\n\ninclude::./response.adoc[]\n\n== Алгоритм работы\nШаг 1.\n\n== Обработка ошибок\n404 - не найдено.\n"
        );
        fs::write(dir.join(format!("{method}.adoc")), main).unwrap();
        fs::write(dir.join("request.adoc"), "${HOST}/api/x\ncurl example").unwrap();
        fs::write(dir.join("response.adoc"), "{}").unwrap();
    }

    #[test]
    fn check_standards_whole_project_reports_per_folder_results() {
        let (repo, docs) = fixture_repo();
        write_full_standards_method(&docs, "getUser");
        fs::create_dir_all(docs.join("createUser")).unwrap();
        fs::write(docs.join("createUser").join("createUser.adoc"), "= createUser").unwrap();

        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps { workspace_index, ..EmbeddingDeps::empty() };

        let result = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs { kind: CheckKind::Standards, path: None }),
            &deps,
            &[],
        )
        .unwrap();
        match result {
            ToolResult::StandardsChecked { report, truncated } => {
                assert!(!truncated);
                assert_eq!(report.folders.len(), 2);
                // Failing folder sorted first.
                assert_eq!(report.folders[0].method_name, "createUser");
                assert!(!report.folders[0].passed);
                assert_eq!(report.folders[1].method_name, "getUser");
                assert!(report.folders[1].passed, "{:?}", report.folders[1]);
                assert!(!report.overall_passed);
            }
            other => panic!("expected StandardsChecked, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn check_standards_scopes_to_method_folder_via_file_path() {
        let (repo, docs) = fixture_repo();
        write_full_standards_method(&docs, "getUser");
        fs::create_dir_all(docs.join("createUser")).unwrap();
        fs::write(docs.join("createUser").join("createUser.adoc"), "= createUser").unwrap();

        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps { workspace_index, ..EmbeddingDeps::empty() };

        // Points at a *file* inside getUser/ — should resolve to that
        // folder only, not the whole docs root.
        let result = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Standards,
                path: Some("getUser/getUser.adoc".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap();
        match result {
            ToolResult::StandardsChecked { report, .. } => {
                assert_eq!(report.folders.len(), 1);
                assert_eq!(report.folders[0].method_name, "getUser");
                assert!(report.folders[0].passed);
            }
            other => panic!("expected StandardsChecked, got {other:?}"),
        }

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn check_standards_rejects_path_escape_under_docs_root() {
        let (repo, docs) = fixture_repo();
        let workspace_index = build_test_workspace_index(&repo);
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let deps = EmbeddingDeps { workspace_index, ..EmbeddingDeps::empty() };

        let err = execute_tool(
            &scope,
            ToolCall::Check(CheckArgs {
                kind: CheckKind::Standards,
                path: Some("../src/main.rs".to_string()),
            }),
            &deps,
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)), "got {err:?}");

        fs::remove_dir_all(&repo).ok();
    }
}
