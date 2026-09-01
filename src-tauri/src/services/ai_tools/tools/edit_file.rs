//! `editFile` — targeted old/new replacements, validated against the
//! file's original content rather than applied one after another, so a
//! batch either lands whole or writes nothing.
//!
//! When an exact match fails, an optional LLM "fast apply" pass reconciles
//! an approximate `old` snippet against the real text. Its output is
//! checked for edit-region safety before anything reaches disk — a model
//! asked to patch one paragraph will occasionally rewrite the document.

use std::sync::Arc;

use crate::domain::ai_tools::{EditFileArgs, FileDiffStats, FileEdit, ToolError, ToolScope};
use crate::domain::asciidoc_macro_brackets::{ClosedMacro, MacroBracketPass};
use crate::domain::llm::{ChatRequest, LlmMessage, LlmProvider, LlmRole, LlmToolDefinition};
use crate::services::{docs_fs, text_diff};

use super::super::EmbeddingDeps;
use super::super::resolve::{reject_atlas_memory_path, resolve_mutable_docs_path};
use super::super::search::truncate_snippet;
use super::write_file::close_adoc_macro_brackets;

/// Same docs-root containment as `write_file`, but composes
/// `docs_fs::read_project_file` + `apply_edits` + `docs_fs::
/// write_project_file` instead of taking new content directly — the file
/// must already exist (a missing file surfaces `read_project_file`'s own
/// `NotFound`, converted via `ToolError`'s `From<ProjectError>`); creating
/// new files stays `write_file`'s job. `fast_apply` is `edit_file`'s own
/// `EmbeddingDeps::fast_apply` field, threaded straight through to
/// `apply_edits`.
pub(super) fn edit_file(
    scope: &ToolScope,
    args: EditFileArgs,
    fast_apply: Option<&(Arc<dyn LlmProvider>, String)>,
    deps: &EmbeddingDeps,
) -> Result<(String, FileDiffStats, Vec<ClosedMacro>), ToolError> {
    let (access_rel, docs_rel) = resolve_mutable_docs_path(scope, &args.path)?;
    reject_atlas_memory_path(scope, &docs_rel)?;
    let docs_root = scope.docs_root.to_string_lossy();
    let content = docs_fs::read_project_file(&docs_root, &docs_rel)?;
    let edited = apply_edits(&content, &args.edits, fast_apply)?;
    // Runs over the whole file, not just the edited regions, so it can also
    // close a bare macro that was already sitting in the file — a line the
    // edits never asked to touch. That is the existing behaviour; reporting
    // `closed` is what stops it from being invisible.
    let MacroBracketPass { content: edited, closed } =
        close_adoc_macro_brackets(&docs_rel, edited);
    docs_fs::write_project_file(&docs_root, &docs_rel, &edited)?;
    // See `write_file`'s matching comment — same best-effort sync.
    let _ = deps.workspace_index.update_document(scope.docs_root.join(&docs_rel));
    let diff = text_diff::diff_stats(&content, &edited);
    Ok((access_rel, diff, closed))
}

/// Applies every edit in `edits` to `content`. The primary path is exact and
/// all-or-nothing, unchanged from before this module had a fast-apply
/// fallback: each edit's `old` is looked up in `content` *as given* — never
/// against the output of an earlier edit in the same call — so edits are
/// independent of each other and of their own order; `old` missing entirely
/// (`EditTextNotFound`), appearing more than once (`EditTextAmbiguous`), or
/// two edits' matched regions overlapping (`EditsOverlap`) all reject the
/// whole call with nothing written, exactly as documented on those
/// `ToolError` variants.
///
/// If that exact pass fails with a per-edit matching problem
/// (`EditTextNotFound`/`EditTextAmbiguous` — *not* `EditsOverlap`, which is a
/// problem with the call itself no amount of reconciliation fixes) and
/// `fast_apply` is `Some`, this falls back to
/// `apply_edits_sequential_with_fallback` instead of failing outright — see
/// its doc comment for how that differs (sequential, not all-at-once). If
/// the fallback itself can't produce a safe result either, its own
/// `EditApplyFailed` (which explains *why* reconciliation failed) is
/// surfaced rather than the plain exact-match error — strictly more useful
/// to a model deciding how to retry, since it confirms reconciliation was
/// attempted at all.
pub(super) fn apply_edits(
    content: &str,
    edits: &[FileEdit],
    fast_apply: Option<&(Arc<dyn LlmProvider>, String)>,
) -> Result<String, ToolError> {
    match apply_edits_exact(content, edits) {
        Ok(result) => Ok(result),
        Err(ToolError::EditTextNotFound(_) | ToolError::EditTextAmbiguous(_, _))
            if fast_apply.is_some() =>
        {
            apply_edits_sequential_with_fallback(content, edits, fast_apply.unwrap())
        }
        Err(err) => Err(err),
    }
}

/// The exact, all-or-nothing matching pass — see `apply_edits`'s doc
/// comment. A pure function with no dependency on `fast_apply`, kept
/// separate so it stays simple to test and reason about on its own.
pub(super) fn apply_edits_exact(content: &str, edits: &[FileEdit]) -> Result<String, ToolError> {
    let ranges = exact_match_ranges(content, edits)?;

    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;
    for (start, end, new) in ranges {
        result.push_str(&content[cursor..start]);
        result.push_str(new);
        cursor = end;
    }
    result.push_str(&content[cursor..]);
    Ok(result)
}

/// Resolves every edit's `old` to a unique `(start, end)` byte range in
/// `content`, sorted by position, after checking none of them overlap.
/// Shared by `apply_edits_exact` (which splices all of them into `content`
/// at once) and `find_unique_exact_match` (which needs just one edit's
/// range, reusing the same not-found/ambiguous checks).
pub(super) fn exact_match_ranges<'a>(
    content: &str,
    edits: &'a [FileEdit],
) -> Result<Vec<(usize, usize, &'a str)>, ToolError> {
    let mut ranges: Vec<(usize, usize, &str)> = Vec::with_capacity(edits.len());
    for edit in edits {
        let (start, end) = find_unique_exact_match(content, &edit.old)?;
        ranges.push((start, end, edit.new.as_str()));
    }

    ranges.sort_by_key(|&(start, _, _)| start);
    for pair in ranges.windows(2) {
        let (_, prev_end, _) = pair[0];
        let (next_start, _, _) = pair[1];
        if next_start < prev_end {
            return Err(ToolError::EditsOverlap);
        }
    }
    Ok(ranges)
}

/// Looks up `old`'s single occurrence in `content` — the same check
/// `apply_edits_exact` runs per edit, extracted so
/// `apply_edits_sequential_with_fallback` can run it one edit at a time
/// against its own, possibly already-modified, view of the content.
pub(super) fn find_unique_exact_match(content: &str, old: &str) -> Result<(usize, usize), ToolError> {
    let mut occurrences = content.match_indices(old);
    let Some((start, _)) = occurrences.next() else {
        return Err(ToolError::EditTextNotFound(old.to_string()));
    };
    let count = 1 + occurrences.count();
    if count > 1 {
        return Err(ToolError::EditTextAmbiguous(old.to_string(), count));
    }
    Ok((start, start + old.len()))
}

/// The fast-apply fallback path: unlike `apply_edits_exact`, edits here are
/// applied one at a time, each against the result of the previous one — not
/// independently against the original content — because a fast-apply call
/// has no fixed byte range to validate for overlap against the others; it
/// can only sensibly operate on "the file as it stands right now". For each
/// edit, an exact match is still tried first (free, deterministic, and
/// correctness-preserving); only an edit that still doesn't match exactly
/// against its current view of the content is escalated to
/// `run_fast_apply`. A model that sent a batch where every edit happens to
/// match exactly sees identical output to `apply_edits_exact`, just reached
/// less directly.
pub(super) fn apply_edits_sequential_with_fallback(
    content: &str,
    edits: &[FileEdit],
    fast_apply: &(Arc<dyn LlmProvider>, String),
) -> Result<String, ToolError> {
    let mut current = content.to_string();
    for edit in edits {
        current = match find_unique_exact_match(&current, &edit.old) {
            Ok((start, end)) => {
                let mut spliced = String::with_capacity(current.len());
                spliced.push_str(&current[..start]);
                spliced.push_str(&edit.new);
                spliced.push_str(&current[end..]);
                spliced
            }
            Err(_) => run_fast_apply(fast_apply, &current, edit)
                .map_err(|reason| ToolError::EditApplyFailed(truncate_snippet(&edit.old), reason))?,
        };
    }
    Ok(current)
}

/// A file larger than this is not sent through fast-apply — the whole
/// content round-trips through the model's context twice over (once in the
/// request, once in the response), so an arbitrarily large document isn't
/// worth the token cost/latency this would add; deterministic matching (or
/// `writeFile` for a rewrite that size) is the right tool past this size.
/// ~40k characters is generous for the documentation files this tool
/// targets (a few thousand lines of prose/markup).
pub(super) const FAST_APPLY_MAX_CONTENT_CHARS: usize = 40_000;

pub(super) const FAST_APPLY_SYSTEM_PROMPT: &str = "You are a precise text-patching engine. You will be given the full current text of a document and one intended edit, expressed as an approximate `old` snippet (it may not match the document's exact current whitespace, line breaks, or formatting) and its `new` replacement. Find the location in the document that the `old` snippet is describing and apply the edit there. Output ONLY the complete resulting document text: every part of the document outside the edited region must be byte-for-byte identical to the input. Do not add any commentary, explanation, or markdown code fences — output the raw document text and nothing else.";

/// Sends `content` plus one edit's intent to the fast-apply model and
/// returns its reconciled full-file output, or an `Err` reason string (never
/// a `ToolError` directly — the caller, `apply_edits_sequential_with_fallback`,
/// wraps it with the edit's own `old` text via `ToolError::EditApplyFailed`).
/// The model's raw output is defensively unwrapped from a markdown code
/// fence if present (`strip_code_fence`) before being checked by
/// `validate_fast_apply_output` — nothing this function returns is ever
/// trusted without that check passing.
pub(super) fn run_fast_apply(
    fast_apply: &(Arc<dyn LlmProvider>, String),
    content: &str,
    edit: &FileEdit,
) -> Result<String, String> {
    if content.chars().count() > FAST_APPLY_MAX_CONTENT_CHARS {
        return Err("file is too large for automatic reconciliation".to_string());
    }
    let (provider, model) = fast_apply;
    let request = ChatRequest {
        model: model.clone(),
        tools: Vec::new(),
        messages: vec![
            LlmMessage {
                role: LlmRole::System,
                content: Some(FAST_APPLY_SYSTEM_PROMPT.to_string()),
                tool_call_id: None,
                tool_calls: vec![],
            },
            LlmMessage {
                role: LlmRole::User,
                content: Some(format!(
                    "FILE CONTENT:\n```\n{content}\n```\n\nREPLACE THIS TEXT:\n```\n{old}\n```\n\nWITH THIS TEXT:\n```\n{new}\n```\n\nOutput the complete updated file content only.",
                    content = content,
                    old = edit.old,
                    new = edit.new,
                )),
                tool_call_id: None,
                tool_calls: vec![],
            },
        ],
    };
    let response = provider.chat(request).map_err(|e| format!("provider error: {e}"))?;
    let raw = response.content.ok_or_else(|| "model returned no content".to_string())?;
    let candidate = strip_code_fence(&raw);
    validate_fast_apply_output(content, &candidate, edit)?;
    Ok(candidate)
}

/// Best-effort removal of a single wrapping markdown code fence — despite
/// `FAST_APPLY_SYSTEM_PROMPT` explicitly asking for raw output, models
/// reliably wrap it in ` ```…``` ` (optionally with a language tag on the
/// opening fence) out of habit. Only strips a fence that wraps the *entire*
/// response (first line is a fence, last line is a fence, at least one line
/// of content between them) — leaves anything else untouched rather than
/// mangling a response that never had a wrapping fence in the first place.
pub(super) fn strip_code_fence(text: &str) -> String {
    let trimmed = text.trim();
    let mut lines: Vec<&str> = trimmed.lines().collect();
    if lines.len() >= 2
        && lines[0].trim_start().starts_with("```")
        && lines[lines.len() - 1].trim() == "```"
    {
        lines.pop();
        lines.remove(0);
        return lines.join("\n");
    }
    text.to_string()
}

/// The fast-apply safety net: `candidate` is only accepted if the model
/// changed nothing outside a bounded region around the intended edit. Finds
/// the longest common prefix and (non-overlapping) longest common suffix
/// between `original` and `candidate`; the unmatched middle on each side is
/// what the model actually changed. Rejects if either middle is implausibly
/// large for the edit that was requested (more than 3x the larger of
/// `old`/`new`'s length, plus a fixed slack for minor reformatting) — a
/// generous bound for a legitimate single-region edit, but well short of
/// what a model rewriting unrelated parts of the file would produce. Also
/// rejects output identical to the input (the model reported success
/// without actually changing anything) or empty output for non-empty input.
pub(super) fn validate_fast_apply_output(original: &str, candidate: &str, edit: &FileEdit) -> Result<(), String> {
    if candidate.is_empty() && !original.is_empty() {
        return Err("model returned an empty file".to_string());
    }
    if candidate == original {
        return Err("model made no change".to_string());
    }

    let original_bytes = original.as_bytes();
    let candidate_bytes = candidate.as_bytes();
    let max_common = original_bytes.len().min(candidate_bytes.len());

    let prefix_len = original_bytes
        .iter()
        .zip(candidate_bytes.iter())
        .take(max_common)
        .take_while(|(a, b)| a == b)
        .count();

    let max_suffix = max_common - prefix_len;
    let suffix_len = original_bytes[prefix_len..]
        .iter()
        .rev()
        .zip(candidate_bytes[prefix_len..].iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();

    let original_middle = original_bytes.len() - prefix_len - suffix_len;
    let candidate_middle = candidate_bytes.len() - prefix_len - suffix_len;

    let cap = edit.old.len().max(edit.new.len()) * 3 + 200;
    if original_middle > cap || candidate_middle > cap {
        return Err(
            "model changed more of the file than the requested edit accounts for — rejected for safety"
                .to_string(),
        );
    }
    Ok(())
}

/// The `editFile` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "editFile".to_string(),
        description:
            "Make one or more precise, targeted edits to an existing documentation file by replacing exact snippets of its current content, given its path relative to the current access-mode root (same as readFile/listFiles). The path must resolve under the documentation tree — paths outside it are rejected with an error. Each edit's `old` text should match the file's CURRENT content exactly once, and all edits in one call are validated against the file's original content and applied together, or none are — they are independent of each other and of their order (atomic application). If an edit's `old` doesn't match exactly (whitespace/formatting drift, or you're recalling the content from memory rather than a fresh read), the call may be rejected; some sessions may attempt automatic reconciliation, but treat exact matching as the contract and add a few more surrounding lines to `old` to make it unique and exact. Prefer this over writeFile for small, localized changes: it's cheaper and safer than resending the whole file. Always requires explicit user approval before anything is written. In AsciiDoc files, block macros are normalized after the edits land, across the whole file rather than only the edited regions: a bare `include::target`, `image::target` or `xref:target` is stored with `[]` appended, because without the brackets AsciiDoc does not read the line as a macro at all. Write the brackets yourself in `new`; the result lists every line changed this way, including any your edits did not touch."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to the current access-mode root (same as readFile). Must resolve under the documentation tree; paths outside it are rejected. Must be a recognized documentation file type and must already exist."
                },
                "edits": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "old": {
                                "type": "string",
                                "description": "Exact text to find — must appear exactly once in the file's current content."
                            },
                            "new": {
                                "type": "string",
                                "description": "Text to replace it with."
                            }
                        },
                        "required": ["old", "new"]
                    },
                    "description": "One or more find-and-replace edits, applied together against the file's original content (atomic), not sequentially against each other's output."
                }
            },
            "required": ["path", "edits"]
        }),
        }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use std::sync::Arc;

    use crate::domain::ai_access::AiAccessMode;
    use crate::domain::ai_tools::{
    EditFileArgs, FileEdit, ToolCall, ToolError, ToolResult, ToolScope,
};
    use crate::services::ai_tools::testing::*;
    use crate::services::ai_tools::{EmbeddingDeps, execute_tool};

    use super::*;

    #[test]
    fn edit_file_full_repo_accepts_docs_relative_path_of_existing_file() {
        let (repo, docs) = fixture_repo();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        edit(&full_repo, "intro.adoc", vec![("= Intro\n", "= Changed\n")]).unwrap();
        assert_eq!(fs::read_to_string(docs.join("intro.adoc")).unwrap(), "= Changed\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_applies_a_single_replacement() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "private String name;\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        edit(
            &scope,
            "class.adoc",
            vec![("private String name;", "private String name;\nprivate int age;")],
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(docs.join("class.adoc")).unwrap(),
            "private String name;\nprivate int age;\n"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_applies_multiple_independent_edits_in_one_call() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\ngamma\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        edit(&scope, "class.adoc", vec![("alpha", "ALPHA"), ("gamma", "GAMMA")]).unwrap();
        assert_eq!(
            fs::read_to_string(docs.join("class.adoc")).unwrap(),
            "ALPHA\nbeta\nGAMMA\n"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_missing_file() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = edit(&scope, "does-not-exist.adoc", vec![("a", "b")]).unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_old_text_not_found_and_writes_nothing() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = edit(&scope, "class.adoc", vec![("nope", "NOPE")]).unwrap_err();
        assert!(matches!(err, ToolError::EditTextNotFound(_)));
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "alpha\nbeta\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_ambiguous_old_text_and_writes_nothing() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "dup\ndup\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = edit(&scope, "class.adoc", vec![("dup", "DUP")]).unwrap_err();
        assert!(matches!(err, ToolError::EditTextAmbiguous(_, 2)));
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "dup\ndup\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_overlapping_edits_and_writes_nothing() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "abcdef\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // "abcd" (0..4) and "cdef" (2..6) overlap on "cd".
        let err = edit(&scope, "class.adoc", vec![("abcd", "X"), ("cdef", "Y")]).unwrap_err();
        assert!(matches!(err, ToolError::EditsOverlap));
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "abcdef\n");

        fs::remove_dir_all(&repo).ok();
    }

    /// Edits are validated against the file's *original* content, not
    /// sequentially against each other's output — an edit whose `new` text
    /// happens to equal another edit's `old` text must not make that other
    /// edit's match count look any different.
    #[test]
    fn edit_file_validates_edits_against_original_content_not_sequentially() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // If edits were applied sequentially, "alpha" -> "beta" would make
        // "beta" ambiguous (two occurrences) by the time the second edit's
        // match is checked. Validated against the original, "beta" still
        // matches exactly once.
        edit(&scope, "class.adoc", vec![("alpha", "beta"), ("beta", "BETA")]).unwrap();
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "beta\nBETA\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_full_repo_mode_still_targets_docs_root_not_repo_root() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("guide.adoc"), "old text\n").unwrap();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        edit(&full_repo, "docs/guide.adoc", vec![("old text", "new text")]).unwrap();
        assert_eq!(fs::read_to_string(docs.join("guide.adoc")).unwrap(), "new text\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_rejects_path_escape() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = edit(&scope, "../outside.adoc", vec![("a", "b")]).unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    fn edit_with_fast_apply(
        scope: &ToolScope,
        path: &str,
        edits: Vec<(&str, &str)>,
        provider: Arc<dyn LlmProvider>,
    ) -> Result<String, ToolError> {
        let deps = EmbeddingDeps {
            fast_apply: Some((provider, "test-model".to_string())),
            ..EmbeddingDeps::empty()
        };
        match execute_tool(
            scope,
            ToolCall::EditFile(EditFileArgs {
                path: path.to_string(),
                edits: edits
                    .into_iter()
                    .map(|(old, new)| FileEdit { old: old.to_string(), new: new.to_string() })
                    .collect(),
            }),
            &deps,
            &[],
        )? {
            ToolResult::FileEdited { path, .. } => Ok(path),
            other => panic!("expected ToolResult::FileEdited, got {other:?}"),
        }
    }

    #[test]
    fn edit_file_fast_apply_reconciles_a_non_exact_old_text() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\ngamma\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // "Beta" (wrong case) never matches "beta" exactly — deterministic
        // matching alone would reject this with `EditTextNotFound`.
        let mock = Arc::new(MockFastApplyProvider::returning("alpha\nBETA\ngamma\n"));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        edit_with_fast_apply(&scope, "class.adoc", vec![("Beta", "BETA")], provider).unwrap();

        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "alpha\nBETA\ngamma\n");
        assert_eq!(mock.calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_fast_apply_is_not_used_when_exact_match_succeeds() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let mock = Arc::new(MockFastApplyProvider::returning("should never be read"));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        edit_with_fast_apply(&scope, "class.adoc", vec![("alpha", "ALPHA")], provider).unwrap();

        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "ALPHA\nbeta\n");
        assert_eq!(mock.calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_fast_apply_strips_a_wrapping_code_fence_from_model_output() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let mock = Arc::new(MockFastApplyProvider::returning("```\nalpha\nBETA\n```"));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        edit_with_fast_apply(&scope, "class.adoc", vec![("Beta", "BETA")], provider).unwrap();

        // No trailing newline: `strip_code_fence` splits on `.lines()`,
        // which discards line-terminator information, so a newline
        // immediately before the closing fence isn't recoverable.
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "alpha\nBETA");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_fast_apply_rejects_output_with_no_change_and_writes_nothing() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let mock = Arc::new(MockFastApplyProvider::returning("alpha\nbeta\n"));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        let err =
            edit_with_fast_apply(&scope, "class.adoc", vec![("Beta", "BETA")], provider).unwrap_err();
        match err {
            ToolError::EditApplyFailed(_, reason) => assert!(reason.contains("no change")),
            other => panic!("expected EditApplyFailed, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "alpha\nbeta\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_fast_apply_rejects_output_that_changes_unrelated_content_and_writes_nothing() {
        let (repo, docs) = fixture_repo();
        let filler = "Unrelated filler sentence stays put. ".repeat(20);
        // Lowercase "beta" so the edit's ("Beta") `old` text does *not*
        // already match exactly — otherwise the deterministic fast path
        // would apply it directly and the mock would never be consulted.
        let original = format!("Intro.\n\nbeta needs fixing.\n\n{filler}\n");
        fs::write(docs.join("class.adoc"), &original).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // The model correctly fixes "Beta" but also rewrites the large
        // unrelated paragraph — far more of the file changed than this
        // edit accounts for, so the whole call must be rejected rather
        // than silently accepting a corrupted rewrite.
        let rewritten_filler = "Something completely different now. ".repeat(20);
        let candidate = format!("Intro.\n\nBETA needs fixing.\n\n{rewritten_filler}\n");
        let mock = Arc::new(MockFastApplyProvider::returning(&candidate));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        let err =
            edit_with_fast_apply(&scope, "class.adoc", vec![("Beta", "BETA")], provider).unwrap_err();
        match err {
            ToolError::EditApplyFailed(_, reason) => assert!(reason.contains("changed more")),
            other => panic!("expected EditApplyFailed, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), original);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_fast_apply_surfaces_a_provider_error() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "alpha\nbeta\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let mock = Arc::new(MockFastApplyProvider::failing("network unreachable"));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        let err =
            edit_with_fast_apply(&scope, "class.adoc", vec![("Beta", "BETA")], provider).unwrap_err();
        match err {
            ToolError::EditApplyFailed(_, reason) => assert!(reason.contains("provider error")),
            other => panic!("expected EditApplyFailed, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "alpha\nbeta\n");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn edit_file_fast_apply_resolves_an_ambiguous_old_text() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("class.adoc"), "dup\ndup\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // "dup" matches twice — deterministic matching alone would reject
        // this with `EditTextAmbiguous`. The model is trusted to pick the
        // right one from context; the safety check only bounds *how much*
        // changed, not *which* occurrence.
        let mock = Arc::new(MockFastApplyProvider::returning("DUP\ndup\n"));
        let provider: Arc<dyn LlmProvider> = mock.clone();
        edit_with_fast_apply(&scope, "class.adoc", vec![("dup", "DUP")], provider).unwrap();

        assert_eq!(fs::read_to_string(docs.join("class.adoc")).unwrap(), "DUP\ndup\n");

        fs::remove_dir_all(&repo).ok();
    }

    /// A canned `LlmProvider` for `EditFile`'s fast-apply fallback tests —
    /// `chat` returns whatever `response` says regardless of the request,
    /// and counts how many times it was called so a test can assert the
    /// fallback was (or, more often, was *not*) actually reached.
    /// `chat_stream`/`list_models` are never used by fast-apply, so they
    /// panic if a bug ever routes a call through them instead.
    struct MockFastApplyProvider {
        response: Result<String, String>,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl MockFastApplyProvider {
        fn returning(content: &str) -> Self {
            Self { response: Ok(content.to_string()), calls: std::sync::atomic::AtomicUsize::new(0) }
        }

        fn failing(message: &str) -> Self {
            Self { response: Err(message.to_string()), calls: std::sync::atomic::AtomicUsize::new(0) }
        }
    }

    impl crate::domain::llm::LlmProvider for MockFastApplyProvider {
        fn chat(
            &self,
            _request: crate::domain::llm::ChatRequest,
        ) -> Result<crate::domain::llm::ChatResponse, crate::domain::llm::LlmError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match &self.response {
                Ok(content) => {
                    Ok(crate::domain::llm::ChatResponse {
                        content: Some(content.clone()),
                        tool_calls: vec![],
                        usage: None,
                    })
                }
                Err(message) => Err(crate::domain::llm::LlmError::Provider(message.clone())),
            }
        }

        fn chat_stream(
            &self,
            _request: crate::domain::llm::ChatRequest,
            _on_delta: &dyn Fn(&str),
            _on_reasoning: &dyn Fn(&str),
            _on_tool_call_delta: &dyn Fn(&str, &str, &str),
            _cancelled: &dyn Fn() -> bool,
        ) -> Result<crate::domain::llm::ChatStreamResult, crate::domain::llm::LlmError> {
            unimplemented!("fast-apply only ever calls chat(), never chat_stream()")
        }

        fn list_models(&self) -> Result<Vec<crate::domain::llm::LlmModelInfo>, crate::domain::llm::LlmError> {
            unimplemented!("fast-apply only ever calls chat(), never list_models()")
        }
    }
}
