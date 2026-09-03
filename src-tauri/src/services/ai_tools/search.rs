//! The matching engine behind `semanticSearch`: symbol lookup, embedding
//! similarity, and BM25 full-text ranking, merged and re-ranked.
//!
//! An exact symbol hit still comes first — it is the cheapest and most
//! precise of the three, and nothing a ranker says outweighs a name the
//! caller spelled correctly. The other two are peers: `fuse_rrf` combines
//! their rankings rather than picking one, because "these words appear
//! here" and "this passage means that" are different kinds of evidence and
//! a chunk carrying both is a better answer than a chunk carrying either.
//! `truncate_snippet` lives here because every match kind renders its
//! preview the same way.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::TryLockError;

use crate::domain::ai_access::AiAccessMode;
use crate::domain::ai_tools::{MatchSource, ToolError, ToolMatch, ToolScope};
use crate::domain::chunk_index::qualified_name_for;
use crate::domain::repo_index::{FileId, Symbol};
use crate::domain::search_query::{
    MatchTightness, extract_search_tokens, fts5_query, fts5_query_from_terms, path_segment_matches,
    symbol_name_matches_token,
};
use crate::infra::{embedding_credentials_store, embedding_providers};
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
    hidden: &mut u32,
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
            *hidden += 1;
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

/// How many BM25 candidates to ask SQLite for per result actually wanted.
/// A hit is dropped after ranking when its chunk is outside the scope
/// (`DocsOnly`), gone from `ChunkIndex`, or no longer resolvable to text —
/// without slack, a `top_k` of 10 could come back with three. Over-fetching
/// inside SQLite costs one bounded query, not a second round trip.
const BM25_OVERFETCH: usize = 8;

/// Ceiling on that over-fetch, so a `MAX_TOP_K` search can't turn into a
/// several-thousand-row result set to throw most of away.
const BM25_MAX_CANDIDATES: usize = 500;

/// The BM25 tier: ranks the FTS5 index in `IndexStore` (see `chunks_fts`)
/// and resolves each hit's chunk text, exactly as the semantic tier does
/// with vector neighbors. Runs off the same persisted store, so it is ready
/// whenever chunks have been indexed — including when no embedding provider
/// is configured at all, which is what makes it the fallback tier.
///
/// `fts` is the model's own list of literal terms
/// (`SemanticSearchArgs::fts`); `None`, or a list that tokenizes to
/// nothing, falls back to tokenizing `query`. The fallback matters: a model
/// that sends `fts: ["…"]` full of punctuation would otherwise lose this
/// tier outright, having asked for *more* precision, not less.
///
/// Infallible by signature on purpose: this is what the cascade degrades
/// *to*, so a store that won't attach, a project that isn't open, or a
/// malformed `MATCH` all report no results rather than failing a search
/// that other tiers can still answer.
pub(super) fn lexical_matches(
    deps: &EmbeddingDeps,
    scope: &ToolScope,
    query: &str,
    fts: Option<&[String]>,
    top_k: usize,
    hidden: &mut u32,
) -> Vec<ToolMatch> {
    let Some(fts_query) = fts
        .and_then(fts5_query_from_terms)
        .or_else(|| fts5_query(query))
    else {
        return Vec::new();
    };

    let store = match attach_current(&deps.chunk_index, &deps.index_store) {
        Ok(Some(attached)) if !attached.stale => attached.store,
        // Stale means the persisted chunking predates this binary — the
        // FTS rows describe spans that no longer line up with the files.
        Ok(_) => return Vec::new(),
        Err(e) => {
            eprintln!("[search] bm25 tier unavailable: {e}");
            return Vec::new();
        }
    };

    let candidates = (top_k * BM25_OVERFETCH).min(BM25_MAX_CANDIDATES);
    let hits = match store.search_bm25(&fts_query, candidates) {
        Ok(hits) => hits,
        Err(e) => {
            eprintln!("[search] bm25 query failed: {e}");
            return Vec::new();
        }
    };

    let mut out = Vec::with_capacity(top_k.min(hits.len()));
    for (chunk_id, score) in hits {
        if out.len() >= top_k {
            break;
        }
        let Some(metadata) = deps.chunk_index.get(&chunk_id) else {
            continue;
        };
        if !scope.allows_search_result(&metadata.file_id) {
            *hidden += 1;
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
            score,
            start_byte: metadata.start_byte,
            end_byte: metadata.end_byte,
            qualified_name: metadata.qualified_name,
            source: MatchSource::Lexical,
        });
    }
    out
}

/// Reciprocal-rank fusion's smoothing constant. 60 is the value the
/// technique was published with and the one everything since uses: large
/// enough that the gap between rank 1 and rank 2 doesn't swamp a second
/// list's opinion, small enough that being ranked at all still matters.
const RRF_K: f32 = 60.0;

/// Merges independently ranked lists into one, scoring each result by
/// `Σ 1/(K + rank)` across the lists that returned it.
///
/// Rank, not score, is what fuses: the tiers measure incomparable things —
/// cosine similarity lands in `0..1`, BM25 in an unbounded positive range
/// that grows with corpus size — and no fixed scale factor makes them
/// commensurable across queries. Ranks are ordinal in both, so a chunk that
/// both tiers rank highly outranks one that a single tier ranks first,
/// which is the whole point: agreement between an exact-term signal and a
/// meaning signal is stronger evidence than either alone.
///
/// Deduped on `(path, start_byte)`, the same identity `symbol_matches`
/// uses. The earliest list's `ToolMatch` is the one kept, so callers order
/// lists by which tier's `source`/snippet should represent a chunk both
/// found.
pub(super) fn fuse_rrf(lists: Vec<Vec<ToolMatch>>, budget: usize) -> Vec<ToolMatch> {
    let mut fused: Vec<ToolMatch> = Vec::new();
    let mut position: std::collections::HashMap<(String, u32), usize> =
        std::collections::HashMap::new();

    for list in lists {
        for (rank, mut candidate) in list.into_iter().enumerate() {
            let contribution = 1.0 / (RRF_K + rank as f32 + 1.0);
            let key = (candidate.path.clone(), candidate.start_byte);
            match position.get(&key) {
                Some(&index) => fused[index].score += contribution,
                None => {
                    candidate.score = contribution;
                    position.insert(key, fused.len());
                    fused.push(candidate);
                }
            }
        }
    }

    fused.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    fused.truncate(budget);
    fused
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
    hidden: &mut u32,
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
                *hidden += 1;
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
                *hidden += 1;
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
                *hidden += 1;
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

        let matches = symbol_matches(&repo_index, &scope, "userservice", 10, &mut 0);
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
            &mut 0,
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
            &mut 0,
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

        let matches = symbol_matches(&repo_index, &scope, "UserService", 10, &mut 0);
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

        assert!(symbol_matches(&repo_index, &scope, "NoSuchSymbol", 10, &mut 0).is_empty());

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
            &mut 0,
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

        let matches = symbol_matches(&repo_index, &scope, "notifications", 10, &mut 0);
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

        assert!(symbol_matches(&repo_index, &scope, "userservice", 10, &mut 0).is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    /// The BM25 tier reads the persisted FTS index, so its tests need a
    /// really-synced project rather than a hand-populated `ChunkIndex`:
    /// runs `files` through `with_open_project` + a full `sync`, then hands
    /// the callback deps sharing that session's slots and a scope in `mode`.
    fn with_synced_project<T>(
        label: &str,
        files: &[(&str, &str)],
        mode: AiAccessMode,
        f: impl FnOnce(&EmbeddingDeps, &ToolScope) -> T,
    ) -> T {
        use crate::services::embedding_state::tests::with_open_project;
        use crate::services::embedding_sync::{ProgressSink, sync};

        with_open_project(label, files, |root, session| {
            let noop: ProgressSink = Arc::new(|_| {});
            sync(session, &noop).unwrap();

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
            // `with_open_project` opens the repo root as both repo and docs
            // root, so `DocsOnly` needs a narrower one to actually exclude
            // anything — `docs/` is where the fixtures put documentation.
            let scope = ToolScope::for_project(root, &root.join("docs"), mode);
            f(&deps, &scope)
        })
    }

    #[test]
    fn lexical_matches_ranks_the_denser_match_first() {
        with_synced_project(
            "bm25-rank",
            &[
                ("dense.adoc", "= Уведомления\n\nПорядок подачи уведомления и сроки рассмотрения уведомления.\n"),
                ("sparse.adoc", "= Общие положения\n\nЗдесь уведомления не рассматриваются.\n"),
            ],
            AiAccessMode::FullRepo,
            |deps, scope| {
                let matches = lexical_matches(deps, scope, "порядок подачи уведомления", None, 10, &mut 0);

                assert_eq!(matches[0].path, "dense.adoc", "{matches:#?}");
                assert_eq!(matches[0].source, MatchSource::Lexical);
                // BM25 is higher-is-better here — `apply_related_boost`
                // multiplies, and a negative score would invert the boost.
                assert!(matches[0].score > 0.0, "{:?}", matches[0].score);
            },
        );
    }

    /// The reason this tier does not reuse `extract_search_tokens`: that
    /// tokenizer keeps only ASCII identifiers, and the corpus is Russian.
    #[test]
    fn lexical_matches_finds_russian_text_the_symbol_tokenizer_would_drop() {
        with_synced_project(
            "bm25-russian",
            &[("guide.adoc", "= Guide\n\nСроки рассмотрения уведомлений по заявке.\n")],
            AiAccessMode::FullRepo,
            |deps, scope| {
                // No ASCII identifier anywhere in the query.
                let matches = lexical_matches(deps, scope, "сроки рассмотрения", None, 10, &mut 0);

                assert_eq!(matches.len(), 1, "{matches:#?}");
                assert!(matches[0].snippet.contains("Сроки"));
            },
        );
    }

    /// `fts` is what the model wants matched literally, so it replaces the
    /// query's own words rather than being added to them — otherwise the
    /// filler it deliberately left out would go on diluting the ranking.
    #[test]
    fn lexical_matches_searches_the_model_supplied_terms_instead_of_the_query() {
        with_synced_project(
            "bm25-fts-arg",
            &[
                ("deadlines.adoc", "= Сроки\n\nСроки рассмотрения заявки.\n"),
                ("registry.adoc", "= Реестр\n\nВедение реестра уведомлений.\n"),
            ],
            AiAccessMode::FullRepo,
            |deps, scope| {
                let query = "где написано про сроки рассмотрения";

                let from_query = lexical_matches(deps, scope, query, None, 10, &mut 0);
                assert_eq!(from_query[0].path, "deadlines.adoc", "{from_query:#?}");

                // Same query, but the model says the word that matters is
                // `реестр` — a word the query never contained.
                let terms = ["реестр".to_string()];
                let from_terms = lexical_matches(deps, scope, query, Some(&terms), 10, &mut 0);

                assert_eq!(from_terms.len(), 1, "{from_terms:#?}");
                assert_eq!(from_terms[0].path, "registry.adoc");
            },
        );
    }

    /// A model that asks for more precision and sends unusable terms must
    /// not end up with less: losing this tier entirely would be the worst
    /// possible answer to `fts: ["—"]`.
    #[test]
    fn lexical_matches_falls_back_to_the_query_when_the_terms_tokenize_to_nothing() {
        with_synced_project(
            "bm25-fts-empty",
            &[("guide.adoc", "= Guide\n\nСроки рассмотрения заявки.\n")],
            AiAccessMode::FullRepo,
            |deps, scope| {
                let terms = ["—".to_string(), "?".to_string()];
                let matches = lexical_matches(deps, scope, "сроки рассмотрения", Some(&terms), 10, &mut 0);

                assert_eq!(matches.len(), 1, "{matches:#?}");
            },
        );
    }

    #[test]
    fn lexical_matches_is_empty_for_an_empty_query() {
        with_synced_project(
            "bm25-empty",
            &[("guide.adoc", "= Guide\n\nтекст\n")],
            AiAccessMode::FullRepo,
            |deps, scope| {
                assert!(lexical_matches(deps, scope, "", None, 10, &mut 0).is_empty());
                assert!(lexical_matches(deps, scope, " — ?! ", None, 10, &mut 0).is_empty());
            },
        );
    }

    #[test]
    fn lexical_matches_excludes_non_doc_chunks_in_docs_only() {
        with_synced_project(
            "bm25-docs-only",
            &[
                ("docs/guide.adoc", "= Guide\n\nуведомления по заявке\n"),
                ("outside.adoc", "= Outside\n\nуведомления по заявке\n"),
            ],
            AiAccessMode::DocsOnly,
            |deps, scope| {
                let mut hidden = 0;
                let matches = lexical_matches(deps, scope, "уведомления", None, 10, &mut hidden);

                assert_eq!(matches.len(), 1, "{matches:#?}");
                assert_eq!(matches[0].path, "guide.adoc");
                assert!(hidden > 0, "the out-of-scope hit must be counted, not swallowed");
            },
        );
    }

    #[test]
    fn lexical_matches_returns_nothing_when_no_project_is_attached() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        // No open project, so no store to rank against — the tier the whole
        // cascade degrades to must degrade quietly itself.
        assert!(lexical_matches(&EmbeddingDeps::empty(), &scope, "needle", None, 10, &mut 0).is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn fuse_rrf_puts_a_chunk_both_tiers_agree_on_above_either_tier_leader() {
        let semantic = vec![sample_match("semantic-only.adoc", 0.9), sample_match("both.adoc", 0.8)];
        let lexical = vec![sample_match("lexical-only.adoc", 12.0), sample_match("both.adoc", 9.0)];

        let fused = fuse_rrf(vec![semantic, lexical], 10);

        // `both` is second in each list — 2/62 — against 1/61 for either
        // list's leader. Corroboration beats a single strong opinion.
        assert_eq!(fused[0].path, "both.adoc", "{fused:#?}");
        assert_eq!(fused.len(), 3, "the shared chunk appears once: {fused:#?}");
    }

    #[test]
    fn fuse_rrf_keeps_the_first_lists_match_for_a_shared_chunk() {
        let mut semantic = sample_match("both.adoc", 0.8);
        semantic.source = MatchSource::Semantic;
        semantic.snippet = "from the semantic tier".to_string();

        let fused = fuse_rrf(vec![vec![semantic], vec![sample_match("both.adoc", 9.0)]], 10);

        // What `meta.has_semantic` reads, and the more informative label
        // for the model.
        assert_eq!(fused[0].source, MatchSource::Semantic);
        assert_eq!(fused[0].snippet, "from the semantic tier");
    }

    #[test]
    fn fuse_rrf_ignores_incomparable_tier_score_scales() {
        // BM25 scores are unbounded and dwarf cosine similarity. Fusing on
        // score rather than rank would hand the lexical list every slot;
        // fusing on rank makes the two lists equal voters.
        let semantic = vec![sample_match("semantic-first.adoc", 0.42)];
        let lexical = vec![sample_match("lexical-first.adoc", 250.0)];

        let fused = fuse_rrf(vec![semantic, lexical], 10);

        assert_eq!(fused[0].score, fused[1].score, "both are rank 1: {fused:#?}");
    }

    #[test]
    fn fuse_rrf_truncates_to_the_budget() {
        let list = vec![sample_match("a.adoc", 1.0), sample_match("b.adoc", 2.0)];
        assert_eq!(fuse_rrf(vec![list], 1).len(), 1);
        assert!(fuse_rrf(Vec::new(), 5).is_empty());
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
