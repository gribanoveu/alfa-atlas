//! `semanticSearch` — the model's default way into an unfamiliar project.
//!
//! The ranking itself lives in `super::super::search`; this module is the
//! tool wrapper around it: budget, readiness check, and the `meta` block
//! that tells the model how much to trust what came back.

use crate::domain::ai_tools::{MatchSource, SemanticSearchArgs, SemanticSearchMeta, SemanticSearchPayload, ToolError, ToolScope};
use crate::domain::llm::LlmToolDefinition;
use crate::domain::search_query::{SearchMetaInput, extract_search_tokens, weak_search_hint};

use super::super::EmbeddingDeps;
use super::super::search::{
    DEFAULT_TOP_K, MAX_TOP_K, apply_related_boost, is_semantic_ready, lexical_matches,
    related_files, semantic_matches, symbol_matches,
};

/// Cascade entry point: an exact symbol-name hit (cheapest, always tried)
/// is prepended to whichever of the semantic/lexical tiers fills the
/// remaining `top_k` budget, chosen by `is_semantic_ready`. Returns matches
/// plus `meta` (extracted tokens, weak-search hint) for the model/UI.
pub(super) fn semantic_search(
    scope: &ToolScope,
    args: SemanticSearchArgs,
    deps: &EmbeddingDeps,
) -> Result<SemanticSearchPayload, ToolError> {
    let top_k = args.top_k.unwrap_or(DEFAULT_TOP_K).clamp(1, MAX_TOP_K);
    let extracted_tokens = extract_search_tokens(&args.query);

    // Exact-name / path-segment tier stays authoritative/unboosted — it's
    // already the cheapest, most-precise signal, not the "did you mean
    // something in the same file family" heuristic `related` below is.
    let mut results = symbol_matches(&deps.repo_index, scope, &args.query, top_k);

    let mut tiers_used = vec!["symbol".to_string()];
    let remaining = top_k.saturating_sub(results.len());
    if remaining > 0 {
        let related = deps
            .active_file
            .as_ref()
            .map(|file_id| related_files(deps, file_id))
            .unwrap_or_default();

        // Over-fetch when a boost could reorder results — a related-but-not-
        // quite-top-ranked chunk needs candidates beyond `remaining` to have any
        // chance of surfacing after boosting.
        let fetch_k = if related.is_empty() {
            remaining
        } else {
            (remaining * 3).min(MAX_TOP_K * 3)
        };

        let tier_results = if is_semantic_ready(deps) {
            tiers_used.push("semantic".to_string());
            semantic_matches(scope, deps, &args.query, fetch_k)?
        } else {
            tiers_used.push("lexical".to_string());
            lexical_matches(&deps.chunk_index, scope, &args.query, fetch_k)
        };

        results.extend(apply_related_boost(tier_results, &related, remaining));
    }

    let symbol_hits = results
        .iter()
        .filter(|m| m.source == MatchSource::Symbol)
        .count() as u32;
    let has_semantic = results.iter().any(|m| m.source == MatchSource::Semantic);
    let only_lexical = !results.is_empty()
        && results.iter().all(|m| m.source == MatchSource::Lexical)
        && symbol_hits == 0;
    let (weak, hint) = weak_search_hint(SearchMetaInput {
        match_count: results.len(),
        symbol_hits,
        has_semantic,
        only_lexical,
        tiers_used: &tiers_used,
        extracted_tokens: &extracted_tokens,
    });

    Ok(SemanticSearchPayload {
        matches: results,
        meta: SemanticSearchMeta {
            tiers_used,
            symbol_hits,
            extracted_tokens,
            weak,
            hint,
        },
    })
}

/// The `semanticSearch` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "semanticSearch".to_string(),
        description:
            "Default search tool — use this first whenever you need to find something in the project and the exact file or line is not already known. Searches via symbol lookup (exact + stem), semantic similarity, and lexical fallback. One strong first query beats several vague repeats — guess camelCase names justified by words in the question (уведомления→Notification/getNotifications, не выдумывать Patent если пользователь не сказал «патент») plus Russian business context; do not send only a lone plain word. Refine with real operation/class names only after a hit reveals them. A second call is only for a new identifier learned from readFile — prefer at most two searches per request. After results, readFile at most 2–3 entry files (adoc + owning *Service); do not listFiles the parent or open mappers/siblings until needed. If meta.hint is present, follow it on the next search. Verify with readFile before precise claims; use grep only for exhaustive exact line matches."
                .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query: English camelCase justified by the question's own words (getNotifications from «уведомления», not invented domain prefixes) + Russian business context. Prefer identifiers over a lone plain word. Strong first query — refine only with new names from readFile."
                },
                "topK": {
                    "type": ["integer", "null"],
                    "minimum": 1,
                    "description": "Max number of results, default 10, capped at 50."
                }
            },
            "required": ["query"]
        }),
        }
}
