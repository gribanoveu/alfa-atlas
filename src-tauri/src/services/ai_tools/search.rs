//! The matching engine behind `semanticSearch`: symbol lookup, embedding
//! similarity, and a lexical fallback, merged and re-ranked.
//!
//! The three run in that order deliberately — an exact symbol hit is worth
//! more than a close embedding, and lexical only fills in when neither
//! found anything. `truncate_snippet` lives here because every match kind
//! renders its preview the same way.

use super::resolve::to_access_relative;
use super::EmbeddingDeps;
use crate::domain::ai_access::AiAccessMode;
use crate::domain::ai_tools::{MatchSource, ToolError, ToolMatch, ToolScope};
use crate::domain::chunk_index::{ChunkMetadata, qualified_name_for};
use crate::domain::repo_index::{FileId, Symbol};
use crate::domain::search_query::{
    MatchTightness, extract_search_tokens, lexical_token_weight, path_segment_matches,
    symbol_name_matches_token,
};
use crate::infra::{embedding_credentials_store, embedding_providers};
use crate::services::{embedding_config, project_open};
use crate::services::chunk_builder::ChunkIndex;
use crate::services::chunk_text::resolve_text;
use crate::services::embedding_state::{
    attach_current, attach_embedding_index, attach_index_store, embedded_count,
    ensure_provider, resolve_index_paths,
};
use crate::services::repo_index::RepositoryIndex;
use std::fs;
use std::collections::HashSet;
use std::path::Path;
use std::sync::TryLockError;

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

    let query_embedding = provider
        .embed(&[query])?
        .into_iter()
        .next()
        .ok_or_else(|| {
            ToolError::SemanticSearch("embedding provider returned no vector".to_string())
        })?;

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
