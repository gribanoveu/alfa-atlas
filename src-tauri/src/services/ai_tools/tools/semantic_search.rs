//! `semanticSearch` — the model's default way into an unfamiliar project.
//!
//! The ranking itself lives in `super::super::search`; this module is the
//! tool wrapper around it: budget, readiness check, and the `meta` block
//! that tells the model how much to trust what came back.

use crate::domain::ai_tools::{MatchSource, SemanticSearchArgs, SemanticSearchMeta, SemanticSearchPayload, ToolError, ToolScope};
use crate::domain::llm::LlmToolDefinition;
use crate::domain::search_query::{SearchMetaInput, extract_search_tokens, weak_search_hint};

use crate::services::embedding_state::embedding_outage_active;

use super::super::EmbeddingDeps;
use super::super::search::{
    DEFAULT_TOP_K, MAX_TOP_K, apply_related_boost, fuse_rrf, is_semantic_ready, lexical_matches,
    related_files, semantic_matches, symbol_matches,
};

/// Entry point: an exact symbol-name hit (cheapest, always tried) is
/// prepended to the recall tiers, which fill the remaining `top_k` budget.
///
/// The recall half used to be a cascade — semantic *or* lexical, whichever
/// `is_semantic_ready` picked. It is now a fusion: BM25 runs on every
/// search and, when the semantic tier is available too, `fuse_rrf` merges
/// the two rankings. The cascade remains only as the degradation path, for
/// when the semantic tier genuinely cannot answer. Returns matches plus
/// `meta` (extracted tokens, weak-search hint) for the model/UI.
pub(super) fn semantic_search(
    scope: &ToolScope,
    args: SemanticSearchArgs,
    deps: &EmbeddingDeps,
) -> Result<SemanticSearchPayload, ToolError> {
    let top_k = args.top_k.unwrap_or(DEFAULT_TOP_K).clamp(1, MAX_TOP_K);
    let extracted_tokens = extract_search_tokens(&args.query);
    // Ranked hits every tier dropped because they sit outside the
    // documentation root — always 0 in Full-repo mode, where nothing is
    // filtered. Reported in `meta` rather than swallowed: to the model an
    // over-filtered search is indistinguishable from an empty repository,
    // and it will happily tell the user the code does not exist.
    let mut hidden = 0u32;

    // Exact-name / path-segment tier stays authoritative/unboosted — it's
    // already the cheapest, most-precise signal, not the "did you mean
    // something in the same file family" heuristic `related` below is.
    let mut results = symbol_matches(&deps.repo_index, scope, &args.query, top_k, &mut hidden);

    let mut tiers_used = vec!["symbol".to_string()];
    let mut degraded: Option<String> = None;
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

        // Always run: one indexed SQLite query, and its exact-term evidence
        // is worth fusing in even when the semantic tier is healthy — an
        // identifier or a Russian term the embedding model blurred is
        // precisely what BM25 is good at.
        let lexical = lexical_matches(deps, scope, &args.query, fetch_k, &mut hidden);
        tiers_used.push("lexical".to_string());

        let tier = choose_tier(is_semantic_ready(deps), embedding_outage_active());
        let tier_results = match tier {
            Tier::Semantic => match semantic_matches(scope, deps, &args.query, fetch_k, &mut hidden) {
                Ok(hits) => {
                    tiers_used.push("semantic".to_string());
                    // Semantic first: `fuse_rrf` keeps the earliest list's
                    // `ToolMatch` for a chunk both tiers found, and
                    // `MatchSource::Semantic` is the more informative label
                    // for the model (and what `meta.has_semantic` reads).
                    fuse_rrf(vec![hits, lexical], fetch_k)
                }
                // The index was ready and the semantic tier still failed —
                // in practice the query-embedding call couldn't reach the
                // provider. The two cheap tiers are fully local and one of
                // them has already produced `results`, so failing the whole
                // call here would throw away working search and leave the
                // model with no discovery tool at all (observed: it then
                // spends the turn on blind `listFiles`/`grep`). Degrade,
                // and say so in `meta` so the answer isn't overtrusted.
                Err(e) => {
                    degraded = Some(degraded_note(&DegradedReason::SemanticFailed(e.to_string())));
                    lexical
                }
            },
            Tier::LexicalDuringOutage => {
                degraded = Some(degraded_note(&DegradedReason::ProviderCoolingDown));
                lexical
            }
            Tier::Lexical => lexical,
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
    // A degradation outranks the ordinary weak-search advice: the standard
    // only-lexical hint tells the model to "wait for embeddings to sync",
    // which is actively wrong when the index is fine and the provider is
    // unreachable — the model would keep re-searching for a tier that
    // cannot come back this turn.
    let hint = degraded.clone().or(hint);

    Ok(SemanticSearchPayload {
        matches: results,
        meta: SemanticSearchMeta {
            tiers_used,
            symbol_hits,
            extracted_tokens,
            weak,
            hint,
            degraded,
            hidden_by_access_boundary: hidden,
        },
    })
}

/// Which tier the cascade's second step should run. Split from
/// `semantic_search` so the "ready but cooling down" case — the one that
/// only shows up with a wall clock and a broken network — is decided by a
/// function that can be tested directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tier {
    Semantic,
    /// Lexical because the semantic tier is *supposed* to work but the
    /// provider recently failed — worth telling the model about.
    LexicalDuringOutage,
    /// Lexical because the index isn't ready (never synced, stale, or a
    /// sync is running) — the long-standing normal case, not a degradation.
    Lexical,
}

fn choose_tier(semantic_ready: bool, outage: bool) -> Tier {
    match (semantic_ready, outage) {
        (true, false) => Tier::Semantic,
        (true, true) => Tier::LexicalDuringOutage,
        (false, _) => Tier::Lexical,
    }
}

enum DegradedReason {
    /// The tier ran and failed; carries the underlying error text.
    SemanticFailed(String),
    /// Skipped without trying, inside the cooldown from a recent failure.
    ProviderCoolingDown,
}

/// What the model reads instead of a silent quality drop. Says which tier
/// is missing, that the results are still usable, and what to do instead —
/// specifically *not* "search again", since a retry cannot restore the
/// semantic tier within this turn.
fn degraded_note(reason: &DegradedReason) -> String {
    let cause = match reason {
        DegradedReason::SemanticFailed(err) => format!("не удалось обратиться к провайдеру эмбеддингов ({err})"),
        DegradedReason::ProviderCoolingDown => {
            "провайдер эмбеддингов недоступен (недавняя ошибка запроса)".to_string()
        }
    };
    format!(
        "Семантический ярус отключён: {cause}. Результаты ниже — только точные имена и текст. \
         Повторный такой же поиск не поможет: уточняйте query точными именами (camelCase) или используйте grep."
    )
}

/// The `semanticSearch` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "semanticSearch".to_string(),
        description:
            "Default search tool — use this first whenever you need to find something in the project and the exact file or line is not already known. Searches via symbol lookup (exact + stem) plus a fusion of semantic similarity and BM25 full-text ranking, so exact wording (Russian included) and meaning both count. One strong first query beats several vague repeats — guess camelCase names justified by words in the question (уведомления→Notification/getNotifications, не выдумывать Patent если пользователь не сказал «патент») plus Russian business context; do not send only a lone plain word. Refine with real operation/class names only after a hit reveals them. A second call is only for a new identifier learned from readFile — prefer at most two searches per request. After results, readFile at most 2–3 entry files (adoc + owning *Service); do not listFiles the parent or open mappers/siblings until needed. If meta.hint is present, follow it on the next search. Verify with readFile before precise claims; use grep only for exhaustive exact line matches."
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The behaviour change this module exists to make: the recall tiers
    /// used to be mutually exclusive, so a healthy semantic index meant BM25
    /// never ran and an exact term in the query contributed nothing.
    #[test]
    fn both_recall_tiers_run_and_fuse_once_the_index_is_ready() {
        use std::sync::Arc;

        use crate::domain::ai_access::AiAccessMode;
        use crate::domain::ai_tools::{ToolCall, ToolResult};
        use crate::services::ai_tools::{EmbeddingDeps, execute_tool};
        use crate::services::embedding_state::tests::with_open_project;
        use crate::services::embedding_sync::{ProgressSink, sync};

        with_open_project(
            "fusion",
            &[("guide.adoc", "= Guide\n\nСроки рассмотрения уведомлений по заявке.\n")],
            |root, session| {
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
                let scope = ToolScope::for_project(root, root, AiAccessMode::FullRepo);

                let result = execute_tool(
                    &scope,
                    ToolCall::SemanticSearch(SemanticSearchArgs {
                        query: "сроки рассмотрения уведомлений".to_string(),
                        top_k: None,
                    }),
                    &deps,
                    &[],
                )
                .unwrap();

                let ToolResult::SemanticSearchResults(payload) = result else {
                    panic!("expected SemanticSearchResults, got {result:?}");
                };
                assert!(
                    payload.meta.tiers_used.contains(&"semantic".to_string())
                        && payload.meta.tiers_used.contains(&"lexical".to_string()),
                    "{:?}",
                    payload.meta.tiers_used
                );
                assert!(payload.meta.degraded.is_none(), "{:?}", payload.meta.degraded);
                assert!(!payload.matches.is_empty());
            },
        );
    }

    #[test]
    fn tier_choice_separates_a_missing_index_from_an_unreachable_provider() {
        assert_eq!(choose_tier(true, false), Tier::Semantic);
        assert_eq!(choose_tier(true, true), Tier::LexicalDuringOutage);
        // Index not ready: lexical, but this is the normal cheap path, not
        // a degradation — nothing to warn the model about.
        assert_eq!(choose_tier(false, false), Tier::Lexical);
        assert_eq!(choose_tier(false, true), Tier::Lexical);
    }

    #[test]
    fn the_degraded_note_names_the_cause_and_steers_away_from_a_retry() {
        let note = degraded_note(&DegradedReason::SemanticFailed(
            "semantic search failed: http error: io: failed to lookup address information".into(),
        ));
        assert!(note.contains("failed to lookup address information"), "{note}");
        assert!(note.contains("grep"), "should offer a usable alternative: {note}");
        assert!(note.contains("не поможет"), "should discourage an identical retry: {note}");

        let cooling = degraded_note(&DegradedReason::ProviderCoolingDown);
        assert!(cooling.contains("недоступен"), "{cooling}");
    }
}
