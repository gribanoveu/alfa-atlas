//! The matching engine behind `semanticSearch`: symbol lookup, embedding
//! similarity, and a lexical fallback, merged and re-ranked.
//!
//! The three run in that order deliberately — an exact symbol hit is worth
//! more than a close embedding, and lexical only fills in when neither
//! found anything. `truncate_snippet` lives here because every match kind
//! renders its preview the same way.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::TryLockError;

use crate::domain::ai_access::AiAccessMode;
use crate::domain::ai_tools::{MatchSource, ToolError, ToolMatch, ToolScope};
use crate::domain::chunk_index::{ChunkMetadata, qualified_name_for};
use crate::domain::repo_index::{FileId, Symbol};
use crate::domain::search_query::{
    MatchTightness, extract_search_tokens, lexical_token_weight, path_segment_matches,
    symbol_name_matches_token,
};
use crate::infra::{embedding_credentials_store, embedding_providers};
use crate::services::chunk_builder::ChunkIndex;
use crate::services::chunk_text::resolve_text;
use crate::services::embedding_state::{
    attach_current, attach_embedding_index, attach_index_store, clear_embedding_outage,
    embedded_count, ensure_provider, note_embedding_outage, resolve_index_paths,
};
use crate::services::repo_index::RepositoryIndex;
use crate::services::{embedding_config, project_open};

use super::EmbeddingDeps;
use super::resolve::to_access_relative;

pub(super) const DEFAULT_TOP_K: usize = 10;

pub(super) const MAX_TOP_K: usize = 50;

/// Cap on how many characters of matched text land in a `ToolMatch.snippet`
/// — keeps a large chunk's (up to 16KB) full text from blowing up the
/// response payload.
pub(super) const SNIPPET_MAX_CHARS: usize = 500;

/// Boosts (multiplicatively, via `RELATED_FILE_BOOST`) every match whose
/// file is in `related`, re-sorts descending by the (possibly boosted)
/// score, then truncates to `budget` — the final step of the cascade's
/// semantic/lexical tier, pulled out as its own pure function so it's
/// testable without going through `is_semantic_ready`'s project-state
/// lookups. A no-op re-sort when `related` is empty would still be correct
/// but is skipped as a cheap early-out, matching `semantic_search`'s
/// pre-existing behavior for "no active file".
pub(super) fn apply_related_boost(
    mut matches: Vec<ToolMatch>,
    related: &HashSet<FileId>,
    budget: usize,
) -> Vec<ToolMatch> {
    if !related.is_empty() {
        for m in &mut matches {
            if related.contains(&FileId(m.path.clone())) {
                m.score *= RELATED_FILE_BOOST;
            }
        }
        matches.sort_by(|a, b| b.score.total_cmp(&a.score));
    }
    matches.truncate(budget);
    matches
}

/// A light nudge (not a hard filter) applied to a search result's score when
/// its file is one `related_files` returns for the currently-open editor
/// tab — multiplicative so it scales sensibly against both the semantic
/// tier's cosine-similarity scores (roughly `0..1`) and the lexical
/// fallback's raw occurrence counts, without needing a tier-specific
/// constant.
pub(super) const RELATED_FILE_BOOST: f32 = 1.25;

/// Combines both dependency graphs `RepositoryIndex`/`WorkspaceIndex`
/// already compute for `file_id`, one hop, forward-only: Java imports (via
/// `RepositoryIndex::java_dependencies`) and AsciiDoc/JSON/YAML
/// includes+`$ref`s (via `WorkspaceIndex::find_includes`/`find_references`).
/// Same combination `commands::embeddings.rs`'s first-sync priority code
/// already performs (`direct_dependencies` + `java_dependencies`), kept as
/// its own small helper here rather than imported — `services` must not
/// depend on `commands`.
pub(super) fn related_files(deps: &EmbeddingDeps, file_id: &FileId) -> HashSet<FileId> {
    let mut out: HashSet<FileId> = deps.repo_index.java_dependencies(file_id).into_iter().collect();

    let doc_id = crate::domain::workspace_index::DocumentId::new(file_id.0.clone());
    for inc in deps.workspace_index.find_includes(&doc_id) {
        out.insert(FileId(inc.path));
    }
    for r in deps.workspace_index.find_references(&doc_id) {
        if !r.target_document.is_empty() {
            out.insert(FileId(r.target_document));
        }
    }
    out
}

/// Whether semantic search can answer right now. Runs the same readiness
/// primitives `embedding_sync::status` does — `attach_current` (project ->
/// index paths -> store attach), a staleness check, then `embedded_count` —
/// so the two can no longer drift apart, plus a `try_lock` peek at
/// `EmbeddingSyncGuard` for "a sync is actively running right now".
///
/// Every failure along that sequence (no project open, a transient
/// store-open error) degrades to "not ready" rather than propagating —
/// consistent with the whole feature being a graceful cascade, not a
/// pipeline that should hard-fail just because the fast path had a hiccup.
/// That degradation is this function's own policy, not the primitives': the
/// guard is likewise never held through this check or the search that
/// follows, unlike `status`, which waits an in-flight sync out.
pub(super) fn is_semantic_ready(deps: &EmbeddingDeps) -> bool {
    // `WouldBlock` (a sync is actively running right now) is the only
    // `try_lock` outcome that should degrade this call — `Poisoned` must
    // not, or a single panic elsewhere while holding this guard (see
    // `services::embedding_state::lock_sync_guard`'s doc comment) would
    // disable semantic search for the rest of the app's lifetime instead of
    // just this one call.
    if matches!(deps.sync_guard.try_lock(), Err(TryLockError::WouldBlock)) {
        return false;
    }

    let Ok(Some(attached)) = attach_current(&deps.chunk_index, &deps.index_store) else {
        return false;
    };
    if attached.stale {
        return false;
    }
    embedded_count(&deps.embedding_index, &attached.store, &attached.index_root)
        .is_ok_and(|n| n > 0)
}

/// Embeds `query`, searches the resident `EmbeddingIndex`, and resolves
/// each hit's chunk text. Independently re-derives `index_root`/attaches
/// the store/index rather than reusing `is_semantic_ready`'s work — cheap
/// and idempotent (each attach short-circuits when already current),
/// matching how `embedding_sync`/`embedding_index_status` each separately
/// re-resolve this instead of sharing state across calls.
pub(super) fn semantic_matches(
    scope: &ToolScope,
    deps: &EmbeddingDeps,
    query: &str,
    top_k: usize,
) -> Result<Vec<ToolMatch>, ToolError> {
    let project = project_open::get_project()
        .map_err(|e| ToolError::SemanticSearch(e.to_string()))?
        .ok_or_else(|| ToolError::SemanticSearch("no project is open".to_string()))?;
    let (index_root, storage_dir) =
        resolve_index_paths(&project).map_err(ToolError::SemanticSearch)?;
    let (store, _stale) = attach_index_store(
        &deps.chunk_index,
        &deps.index_store,
        &storage_dir,
        &index_root,
    )
    .map_err(ToolError::SemanticSearch)?;

    let config = embedding_config::resolve_embedding_config()
        .map_err(|e| ToolError::SemanticSearch(e.to_string()))?;
    let dimensions = embedding_providers::expected_dimensions(&config);
    attach_embedding_index(&deps.embedding_index, &store, &index_root, dimensions, false)
        .map_err(ToolError::SemanticSearch)?;

    let api_key = embedding_credentials_store::get_api_key();
    let provider = ensure_provider(&deps.embedding_provider, &config, api_key)
        .map_err(ToolError::SemanticSearch)?;

    // The only network call in a search: the *index* holds document
    // vectors, but the query still has to be embedded now, by the same
    // model — so a `Remote` provider makes every search depend on the
    // endpoint being reachable, however complete the index is. Failures
    // here open a short cooldown (`note_embedding_outage`) so the rest of
    // the session falls straight through to the lexical tier instead of
    // paying a connect timeout per search; the caller
    // (`tools::semantic_search`) is what turns this error into that
    // fallback rather than a failed tool call.
    let query_embedding = match provider.embed(&[query]) {
        Ok(vectors) => {
            clear_embedding_outage();
            vectors.into_iter().next().ok_or_else(|| {
                ToolError::SemanticSearch("embedding provider returned no vector".to_string())
            })?
        }
        Err(e) => {
            note_embedding_outage();
            return Err(ToolError::from(e));
        }
    };

    let hits = {
        let slot = deps.embedding_index.lock().map_err(|_| {
            ToolError::SemanticSearch("embedding index lock poisoned".to_string())
        })?;
        let Some((_, _, index)) = slot.as_ref() else {
            return Ok(Vec::new());
        };
        // This `usearch` wrapper has no predicate-aware ANN search — when
        // this scope filters results (`DocsOnly`), over-fetch the whole
        // corpus so filtering below can still fill `top_k` from whatever's
        // left, rather than silently returning fewer hits than the caller
        // asked for just because the nearest raw neighbors happened to be
        // outside `docs_root`.
        let search_k = if scope.mode == AiAccessMode::DocsOnly {
            top_k.max(index.len())
        } else {
            top_k
        };
        index.search(&query_embedding, search_k)?
    };

    let mut out = Vec::with_capacity(top_k.min(hits.len()));
    for (chunk_id, distance) in hits {
        if out.len() >= top_k {
            break;
        }
        let Some(metadata) = deps.chunk_index.get(&chunk_id) else {
            continue;
        };
        if !scope.allows_search_result(&metadata.file_id) {
            continue;
        }
        let Ok(text) = resolve_text(&scope.repo_root, &metadata) else {
            continue;
        };
        let Some(access_path) = to_access_relative(scope, &metadata.file_id.0) else {
            continue;
        };
        out.push(ToolMatch {
            path: access_path,
            snippet: truncate_snippet(&text),
            // `EmbeddingIndex::search` returns cosine distance (lower is
            // closer) — flip to a "higher is better" similarity score.
            score: 1.0 - distance,
            start_byte: metadata.start_byte,
            end_byte: metadata.end_byte,
            qualified_name: metadata.qualified_name,
            source: MatchSource::Semantic,
        });
    }
    Ok(out)
}

/// No-embeddings fallback: scans every chunk's resolved text for
/// case-insensitive token matches (from `extract_search_tokens`), ranked by
/// a weighted occurrence sum. When no tokens are extracted, falls back to
/// the whole query as a single needle (backward-compatible for one-word
/// queries). Scores are not comparable to the semantic tier's cosine
/// similarity.
pub(super) fn lexical_matches(
    chunk_index: &ChunkIndex,
    scope: &ToolScope,
    query: &str,
    top_k: usize,
) -> Vec<ToolMatch> {
    let tokens = extract_search_tokens(query);
    let needles: Vec<(String, f32)> = if tokens.is_empty() {
        let whole = query.trim().to_lowercase();
        if whole.is_empty() {
            return Vec::new();
        }
        vec![(whole, 1.0)]
    } else {
        tokens
            .iter()
            .map(|t| (t.to_lowercase(), lexical_token_weight(t)))
            .collect()
    };

    let mut scored: Vec<(f32, ChunkMetadata, String)> = Vec::new();
    for metadata in chunk_index.all() {
        if !scope.allows_search_result(&metadata.file_id) {
            continue;
        }
        let Ok(text) = resolve_text(&scope.repo_root, &metadata) else {
            continue;
        };
        let lower = text.to_lowercase();
        let mut score = 0.0_f32;
        for (needle, weight) in &needles {
            let count = lower.matches(needle.as_str()).count() as f32;
            score += count * weight;
        }
        if score > 0.0 {
            scored.push((score, metadata, text));
        }
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(top_k);

    scored
        .into_iter()
        .filter_map(|(score, metadata, text)| {
            let access_path = to_access_relative(scope, &metadata.file_id.0)?;
            Some(ToolMatch {
                path: access_path,
                snippet: truncate_snippet(&text),
                score,
                start_byte: metadata.start_byte,
                end_byte: metadata.end_byte,
                qualified_name: metadata.qualified_name,
                source: MatchSource::Lexical,
            })
        })
        .collect()
}

/// Score for an exact symbol-name hit.
pub(super) const SYMBOL_NAME_SCORE: f32 = 1.0;

/// Score for a stem/fuzzy symbol-name hit (e.g. notifications ⊂ NotificationService).
pub(super) const SYMBOL_STEM_SCORE: f32 = 0.95;

/// Score for a path-segment match (slightly below stem).
pub(super) const SYMBOL_PATH_SCORE: f32 = 0.9;

/// Cheapest tier: exact (case-insensitive) symbol-name matches for each
/// extracted token, stem/fuzzy symbol matches, plus path-segment matches
/// against indexed file paths. Always tried first, regardless of whether
/// the embedding index is ready.
pub(super) fn symbol_matches(
    repo_index: &RepositoryIndex,
    scope: &ToolScope,
    query: &str,
    top_k: usize,
) -> Vec<ToolMatch> {
    let mut tokens = extract_search_tokens(query);
    // Backward compat: a single exact name with no separators still works
    // even when extract yields nothing unusual (e.g. "UserService" is
    // PascalCase and is extracted; "userservice" all-lowercase plain ≥ 3
    // is also extracted). If the whole trimmed query is a single ASCII
    // word not already listed, include it so `find_symbol` still sees it.
    let trimmed = query.trim();
    if !trimmed.is_empty()
        && trimmed.chars().all(|c| c.is_ascii_alphanumeric())
        && !tokens.iter().any(|t| t.eq_ignore_ascii_case(trimmed))
    {
        tokens.push(trimmed.to_string());
    }
    if tokens.is_empty() {
        return Vec::new();
    }

    // Dedupe key: (access_path, start_byte). Exact (1.0) > stem (0.95) >
    // path (0.9) for the same range/file.
    let mut best: std::collections::HashMap<(String, u32), ToolMatch> =
        std::collections::HashMap::new();

    let insert_candidate =
        |best: &mut std::collections::HashMap<(String, u32), ToolMatch>, candidate: ToolMatch| {
            let key = (candidate.path.clone(), candidate.start_byte);
            best.entry(key)
                .and_modify(|existing| {
                    if candidate.score > existing.score {
                        *existing = candidate.clone();
                    }
                })
                .or_insert(candidate);
        };

    // Exact name lookups (fast path via index).
    for token in &tokens {
        for (file_id, symbol) in repo_index.find_symbol(token) {
            if !scope.allows_search_result(&file_id) {
                continue;
            }
            let Some(access_path) = to_access_relative(scope, &file_id.0) else {
                continue;
            };
            let all_symbols = match repo_index.get(&file_id) {
                Some(f) => f.symbols,
                None => continue,
            };
            let qualified_name =
                qualified_name_for(&symbol, &all_symbols).or_else(|| Some(symbol.name.clone()));
            let snippet =
                read_symbol_snippet(&scope.repo_root, &file_id, &symbol).unwrap_or_default();
            insert_candidate(
                &mut best,
                ToolMatch {
                    path: access_path,
                    snippet,
                    score: SYMBOL_NAME_SCORE,
                    start_byte: symbol.start_byte,
                    end_byte: symbol.end_byte,
                    qualified_name,
                    source: MatchSource::Symbol,
                },
            );
        }
    }

    // Stem/fuzzy: walk indexed symbols once per token that wasn't an exact
    // hit for every possible name (notifications → CollectNotificationService).
    for token in &tokens {
        for (file_id, indexed) in repo_index.all_files() {
            if !scope.allows_search_result(&file_id) {
                continue;
            }
            let Some(access_path) = to_access_relative(scope, &file_id.0) else {
                continue;
            };
            for symbol in &indexed.symbols {
                match symbol_name_matches_token(&symbol.name, token) {
                    MatchTightness::None | MatchTightness::Exact => continue,
                    MatchTightness::Stem => {}
                }
                let qualified_name = qualified_name_for(symbol, &indexed.symbols)
                    .or_else(|| Some(symbol.name.clone()));
                let snippet =
                    read_symbol_snippet(&scope.repo_root, &file_id, symbol).unwrap_or_default();
                insert_candidate(
                    &mut best,
                    ToolMatch {
                        path: access_path.clone(),
                        snippet,
                        score: SYMBOL_STEM_SCORE,
                        start_byte: symbol.start_byte,
                        end_byte: symbol.end_byte,
                        qualified_name,
                        source: MatchSource::Symbol,
                    },
                );
            }
        }
    }

    // Path-segment matches — only for files not already covered by a
    // symbol hit at any byte range.
    for token in &tokens {
        for (file_id, _indexed) in repo_index.all_files() {
            if !scope.allows_search_result(&file_id) {
                continue;
            }
            if !path_segment_matches(&file_id.0, token) {
                continue;
            }
            let Some(access_path) = to_access_relative(scope, &file_id.0) else {
                continue;
            };
            if best.keys().any(|(p, _)| p == &access_path) {
                continue;
            }
            let snippet =
                read_file_first_line_snippet(&scope.repo_root, &file_id).unwrap_or_default();
            insert_candidate(
                &mut best,
                ToolMatch {
                    path: access_path,
                    snippet,
                    score: SYMBOL_PATH_SCORE,
                    start_byte: 0,
                    end_byte: 0,
                    qualified_name: None,
                    source: MatchSource::Symbol,
                },
            );
        }
    }

    let mut out: Vec<ToolMatch> = best.into_values().collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    out.truncate(top_k);
    out
}

/// Best-effort first line of a file for path-segment symbol hits.
pub(super) fn read_file_first_line_snippet(scope_root: &Path, file_id: &FileId) -> Option<String> {
    let path = scope_root.join(&file_id.0);
    let content = fs::read_to_string(&path).ok()?;
    let line = content.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return None;
    }
    Some(truncate_snippet(line))
}

/// Best-effort slice of `[symbol.start_byte..symbol.end_byte)` off whatever
/// is on disk right now. Unlike `chunk_text::resolve_text`, there's no
/// per-symbol content hash to check staleness against (`Symbol` carries no
/// hash, only `IndexedFile.metadata.hash` does, at the whole-file level) —
/// this simply returns `None` (dropping the snippet, not the whole match)
/// if the byte range is no longer valid for the file's current content.
pub(super) fn read_symbol_snippet(scope_root: &Path, file_id: &FileId, symbol: &Symbol) -> Option<String> {
    let path = scope_root.join(&file_id.0);
    let content = fs::read_to_string(&path).ok()?;
    let start = symbol.start_byte as usize;
    let end = symbol.end_byte as usize;
    if end > content.len()
        || start > end
        || !content.is_char_boundary(start)
        || !content.is_char_boundary(end)
    {
        return None;
    }
    Some(truncate_snippet(&content[start..end]))
}

pub(super) fn truncate_snippet(text: &str) -> String {
    if text.len() <= SNIPPET_MAX_CHARS {
        return text.to_string();
    }
    let mut end = SNIPPET_MAX_CHARS;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use std::fs;

    use std::sync::Arc;

    use std::collections::HashSet;

    use crate::services::workspace_index::WorkspaceIndex;

    use crate::domain::ai_access::AiAccessMode;
    use crate::domain::ai_tools::{MatchSource, ToolMatch, ToolScope};
    use crate::services::ai_tools::testing::*;
    use crate::services::ai_tools::EmbeddingDeps;

    use super::*;

    #[test]
    fn related_files_combines_java_imports_and_workspace_includes() {
        let (repo, docs) = fixture_repo();

        // JSON `$ref` side: `current.json` -> `related.json`.
        fs::write(docs.join("current.json"), r#"{"$ref": "./related.json"}"#).unwrap();
        fs::write(docs.join("related.json"), "{}").unwrap();

        let workspace_index =
            Arc::new(WorkspaceIndex::new(crate::infra::parsers::registry::ParserRegistry::new()));
        workspace_index.build(repo.clone()).unwrap();

        // Java side: `Current.java` imports `com.example.Related` —
        // `java_dependencies` matches on the literal on-disk path, so the
        // package directory layout must actually match `com/example/`.
        let pkg = repo.join("src/com/example");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("Current.java"), "import com.example.Related;\nclass Current {}\n").unwrap();
        fs::write(pkg.join("Related.java"), "package com.example;\nclass Related {}\n").unwrap();
        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();

        let deps = EmbeddingDeps {
            workspace_index,
            repo_index: Arc::new(repo_index),
            ..EmbeddingDeps::empty()
        };

        let json_related = related_files(&deps, &FileId("docs/current.json".to_string()));
        assert!(json_related.contains(&FileId("docs/related.json".to_string())));

        let java_related = related_files(&deps, &FileId("src/com/example/Current.java".to_string()));
        assert!(java_related.contains(&FileId("src/com/example/Related.java".to_string())));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn related_files_is_empty_for_an_unknown_file() {
        let deps = EmbeddingDeps::empty();
        assert!(related_files(&deps, &FileId("nowhere.json".to_string())).is_empty());
    }

    fn sample_match(path: &str, score: f32) -> ToolMatch {
        ToolMatch {
            path: path.to_string(),
            snippet: String::new(),
            score,
            start_byte: 0,
            end_byte: 0,
            qualified_name: None,
            source: MatchSource::Lexical,
        }
    }

    #[test]
    fn apply_related_boost_reorders_a_related_match_above_a_stronger_unrelated_one() {
        let matches = vec![sample_match("unrelated.json", 6.0), sample_match("related.json", 5.0)];
        let related: HashSet<FileId> = [FileId("related.json".to_string())].into_iter().collect();

        // `5.0 * RELATED_FILE_BOOST` (`1.25`) = `6.25`, just enough to edge
        // out the unboosted `6.0`.
        let boosted = apply_related_boost(matches, &related, 2);

        assert_eq!(boosted[0].path, "related.json");
        assert_eq!(boosted[1].path, "unrelated.json");
    }

    #[test]
    fn apply_related_boost_is_a_no_op_with_no_related_files() {
        let matches = vec![sample_match("a.json", 6.0), sample_match("b.json", 5.0)];

        let unboosted = apply_related_boost(matches, &HashSet::new(), 2);

        assert_eq!(unboosted[0].path, "a.json");
        assert_eq!(unboosted[0].score, 6.0);
        assert_eq!(unboosted[1].path, "b.json");
        assert_eq!(unboosted[1].score, 5.0);
    }

    #[test]
    fn apply_related_boost_truncates_to_budget_after_resorting() {
        let matches = vec![sample_match("unrelated.json", 6.0), sample_match("related.json", 5.0)];
        let related: HashSet<FileId> = [FileId("related.json".to_string())].into_iter().collect();

        let boosted = apply_related_boost(matches, &related, 1);

        assert_eq!(boosted.len(), 1);
        assert_eq!(boosted[0].path, "related.json");
    }

    #[test]
    fn symbol_matches_finds_an_exact_case_insensitive_name() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/UserService.java"),
            "public class UserService {\n    public String getName() { return null; }\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(&repo_index, &scope, "userservice", 10);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].source, MatchSource::Symbol);
        assert!(matches[0].path.ends_with("UserService.java"));
        assert_eq!(matches[0].score, 1.0);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_extracts_tokens_from_natural_language_query() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/CollectNotificationService.java"),
            "public class CollectNotificationService {\n    public void run() {}\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(
            &repo_index,
            &scope,
            "алгоритм формирования списка уведомлений для подачи notifications",
            10,
        );
        assert!(!matches.is_empty());
        assert!(matches.iter().any(|m| m.path.contains("CollectNotificationService")));
        assert!(matches.iter().all(|m| m.source == MatchSource::Symbol));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_finds_multiple_identifiers_in_one_query() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/CollectNotificationService.java"),
            "public class CollectNotificationService {}\n",
        )
        .unwrap();
        fs::write(
            docs.join("getPatentNotifications.adoc"),
            "= getPatentNotifications\n",
        )
        .unwrap();
        // AsciiDoc section may or may not index as a symbol named
        // getPatentNotifications — path match still covers the folder/file.
        fs::create_dir_all(docs.join("getPatentNotifications")).unwrap();
        fs::write(
            docs.join("getPatentNotifications/getPatentNotifications.adoc"),
            "= Method\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(
            &repo_index,
            &scope,
            "CollectNotificationService getPatentNotifications",
            10,
        );
        assert!(matches.iter().any(|m| m.path.contains("CollectNotificationService")));
        assert!(matches.iter().any(|m| m.path.contains("getPatentNotifications")));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_dedupes_symbol_over_path_for_same_file() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/UserService.java"),
            "public class UserService {\n    public String getName() { return null; }\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(&repo_index, &scope, "UserService", 10);
        let path_hits: Vec<_> = matches
            .iter()
            .filter(|m| m.path.ends_with("UserService.java"))
            .collect();
        assert_eq!(path_hits.len(), 1);
        assert_eq!(path_hits[0].score, 1.0);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_is_empty_for_an_unknown_name() {
        let (repo, docs) = fixture_repo();
        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        assert!(symbol_matches(&repo_index, &scope, "NoSuchSymbol", 10).is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_cyrillic_only_query_finds_via_ru_en() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/CollectNotificationService.java"),
            "public class CollectNotificationService {}\n",
        )
        .unwrap();
        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(
            &repo_index,
            &scope,
            "алгоритм формирования списка уведомлений",
            10,
        );
        assert!(
            matches.iter().any(|m| m.path.contains("CollectNotificationService")),
            "RU→EN Notification + stem should find CollectNotificationService"
        );

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_stem_finds_notification_service() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/CollectNotificationService.java"),
            "public class CollectNotificationService {}\n",
        )
        .unwrap();
        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = symbol_matches(&repo_index, &scope, "notifications", 10);
        assert!(matches.iter().any(|m| m.path.contains("CollectNotificationService")));
        assert!(matches.iter().any(|m| (m.score - 0.95).abs() < 0.01 || m.score >= 0.95));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn symbol_matches_excludes_non_doc_symbols_in_docs_only() {
        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/UserService.java"),
            "public class UserService {\n    public String getName() { return null; }\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        assert!(symbol_matches(&repo_index, &scope, "userservice", 10).is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_finds_a_case_insensitive_substring() {
        use crate::domain::chunk_index::ChunkBuildOptions;
        use crate::services::chunk_builder::ChunkBuilder;

        let (repo, docs) = fixture_repo();
        fs::write(repo.join("docs/needle.adoc"), "= Guide\n\nfind the NEEDLE here\n").unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(ChunkBuilder::new().build_all(&repo_index, &ChunkBuildOptions::default()));
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = lexical_matches(&chunk_index, &scope, "needle", 10);
        assert!(!matches.is_empty());
        assert_eq!(matches[0].source, MatchSource::Lexical);
        assert!(matches[0].snippet.to_lowercase().contains("needle"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_tokenizes_natural_language_query() {
        use crate::domain::chunk_index::ChunkBuildOptions;
        use crate::services::chunk_builder::ChunkBuilder;

        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("docs/guide.adoc"),
            "= Guide\n\nHere we describe notifications for patent submit.\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(ChunkBuilder::new().build_all(&repo_index, &ChunkBuildOptions::default()));
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let matches = lexical_matches(
            &chunk_index,
            &scope,
            "алгоритм формирования списка уведомлений notifications",
            10,
        );
        assert!(!matches.is_empty());
        assert!(matches[0].snippet.to_lowercase().contains("notifications"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_is_empty_for_an_empty_query() {
        let (repo, docs) = fixture_repo();
        let chunk_index = ChunkIndex::new();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        assert!(lexical_matches(&chunk_index, &scope, "", 10).is_empty());
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn lexical_matches_excludes_non_doc_chunks_in_docs_only() {
        use crate::domain::chunk_index::ChunkBuildOptions;
        use crate::services::chunk_builder::ChunkBuilder;

        let (repo, docs) = fixture_repo();
        fs::write(
            repo.join("src/Needle.java"),
            "public class Needle {\n    // find the NEEDLE here\n}\n",
        )
        .unwrap();

        let repo_index = RepositoryIndex::new();
        repo_index.build(&repo).unwrap();
        let chunk_index = ChunkIndex::new();
        chunk_index.insert_all(ChunkBuilder::new().build_all(&repo_index, &ChunkBuildOptions::default()));
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        assert!(lexical_matches(&chunk_index, &scope, "needle", 10).is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    /// Regression guard for the readiness de-duplication.
    ///
    /// `is_semantic_ready` and `embedding_sync::status` used to be two
    /// hand-maintained copies of the same four-step sequence — this file's
    /// copy literally opened with "Mirrors `embedding_index_status`'s
    /// readiness check exactly". They now share `attach_current` +
    /// `embedded_count`, and this pins the property that comment was
    /// asserting on trust: on identical state, the two must agree.
    #[test]
    fn semantic_readiness_agrees_with_the_reported_index_status() {
        use crate::services::embedding_state::tests::with_open_project;
        use crate::services::embedding_sync::{ProgressSink, status, sync};

        with_open_project(
            "semantic-readiness",
            &[("a.json", "{\"a\": 1}")],
            |_root, session| {
                let noop: ProgressSink = Arc::new(|_| {});
                // Shares the session's slots rather than fresh ones — the two
                // paths must be looking at the same state for the comparison
                // to mean anything.
                let deps = EmbeddingDeps {
                    repo_index: session.repo_index.clone(),
                    chunk_index: session.chunk_index.clone(),
                    embedding_index: session.embedding_index.clone(),
                    index_store: session.index_store.clone(),
                    embedding_provider: session.embedding_provider.clone(),
                    sync_guard: session.sync_guard.clone(),
                    workspace_index: session.workspace_index.clone(),
                    fast_apply: None,
                    active_file: None,
                };

                assert_eq!(
                    is_semantic_ready(&deps),
                    status(session, &noop).unwrap().synced,
                    "before any sync, both paths should report not-ready"
                );

                sync(session, &noop).unwrap();

                assert_eq!(
                    is_semantic_ready(&deps),
                    status(session, &noop).unwrap().synced,
                    "after a sync, both paths should report ready"
                );
                assert!(is_semantic_ready(&deps), "the sync did embed something");
            },
        );
    }
}
