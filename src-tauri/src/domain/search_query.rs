//! Pure helpers for `SemanticSearch`: token extraction from a natural-language
//! query, and weak-search hint text. No I/O — `services::ai_tools` owns the
//! cascade that consumes these tokens; `domain::ai_tools` owns the payload
//! types that carry the resulting meta.

use std::collections::HashSet;

/// Minimum length for a plain ASCII word token (shorter ones are noise).
const MIN_PLAIN_TOKEN_LEN: usize = 3;

/// Minimum stem length for fuzzy symbol / path matching.
pub const MIN_STEM_LEN: usize = 4;

/// Priority band for sorting extracted tokens — higher first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TokenKind {
    Plain = 0,
    PathLike = 1,
    CamelOrPascal = 2,
}

/// Small RU→EN expansions for common documentation roots. Deliberately
/// narrow — false positives from a large dictionary would pollute symbol
/// search more than they help.
const RU_EN_ROOTS: &[(&str, &str)] = &[
    ("уведомлен", "Notification"),
    ("патент", "Patent"),
    ("валидац", "Validation"),
    ("авториз", "Authorization"),
    ("аутентифик", "Authentication"),
];

/// Extract search tokens from a free-form query: camelCase/PascalCase
/// identifiers, path-like segments, plain ASCII words (≥ 3 chars), and a
/// few RU→EN expansions from Cyrillic roots. Deduped case-insensitively;
/// ordered Camel/Pascal > path-like > plain.
pub fn extract_search_tokens(query: &str) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut scored: Vec<(TokenKind, String)> = Vec::new();

    for raw in split_raw_segments(query) {
        if raw.is_empty() {
            continue;
        }
        // Path-like: contains / - _ . and is mostly ASCII identifier chars.
        if is_path_like(&raw) {
            push_token(&mut seen, &mut scored, TokenKind::PathLike, &raw);
            // Also try the basename without extension.
            if let Some(stem) = path_stem(&raw) {
                classify_and_push(&mut seen, &mut scored, stem);
            }
            continue;
        }
        classify_and_push(&mut seen, &mut scored, &raw);
    }

    // RU→EN: scan the whole query (lowercase) for known roots.
    let query_lower = query.to_lowercase();
    for (ru_root, en) in RU_EN_ROOTS {
        if query_lower.contains(ru_root) {
            // Prefer PascalCase form for symbol tier.
            push_token(&mut seen, &mut scored, TokenKind::CamelOrPascal, en);
        }
    }

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.len().cmp(&b.1.len()).reverse()));
    scored.into_iter().map(|(_, t)| t).collect()
}

/// Terms past this are dropped. A pasted paragraph would otherwise build an
/// FTS5 query with hundreds of `OR` branches, every one of them a term
/// lookup, for words that barely move a BM25 score anyway — the ones that
/// do are the rare ones, and a query usually leads with those.
const MAX_FTS_TERMS: usize = 24;

/// Below this a term is punctuation-level noise (`и`, `в`, `a`).
const MIN_FTS_TERM_CHARS: usize = 2;

/// From this length up a term is searched as a prefix. Shorter than four
/// characters a prefix matches so much that it stops being evidence.
const FTS_PREFIX_MIN_CHARS: usize = 4;

/// Longer terms are cut to this many characters before the prefix is
/// applied: `уведомления` is searched as `уведом*`.
///
/// This is the stand-in for a stemmer, and it has to cut rather than just
/// append `*` because Russian inflects the *suffix* — `уведомления` is not
/// a prefix of `уведомлений`, so the untruncated prefix term would miss the
/// very case it exists for. Cutting to a fixed length works because Russian
/// (and English, and camelCase identifiers) keep the stem at the front:
/// `уведом*` reaches every case of `уведомление`, `notifi*` reaches
/// `notification`/`notifications`.
///
/// Six is the balance point. Shorter starts merging unrelated words;
/// longer starts missing inflections again — Russian endings run to three
/// characters. The recall this buys is worth more than the precision it
/// costs, because BM25's own term weighting already discounts whatever a
/// loose prefix drags in: a prefix matching many terms matches common ones,
/// and common terms score near zero.
///
/// The `prefix` index in `chunks_fts` is built for exactly the lengths this
/// can emit (4, 5, 6) — keep the two in step.
const FTS_STEM_PREFIX_CHARS: usize = 6;

/// Builds the `MATCH` expression for the BM25 tier — a separate tokenizer
/// from `extract_search_tokens` on purpose. That one feeds the *symbol*
/// tier and deliberately keeps only ASCII identifiers, dropping Cyrillic
/// entirely (see `split_raw_segments`); the indexed corpus here is Russian
/// documentation, so reusing it would throw away the half of the query that
/// actually matches the text.
///
/// Terms are joined with `OR`, not FTS5's implicit `AND`: a
/// natural-language question shares only some of its words with any one
/// chunk, and requiring all of them returns nothing. BM25 then does the
/// discriminating — a chunk matching more, rarer terms outranks one
/// matching a single common word.
///
/// `None` when nothing survives tokenization (empty or punctuation-only
/// query) — there is no such thing as an empty `MATCH` expression.
pub fn fts5_query(query: &str) -> Option<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut terms: Vec<String> = Vec::new();

    for raw in query.split(|c: char| !c.is_alphanumeric()) {
        if terms.len() >= MAX_FTS_TERMS {
            break;
        }
        let lower = raw.to_lowercase();
        let chars = lower.chars().count();
        if chars < MIN_FTS_TERM_CHARS {
            continue;
        }
        // Always quoted. Bare `AND`/`OR`/`NOT`/`NEAR` read as operators and
        // a bare leading digit is a syntax error, so an unquoted user word
        // can fail the whole query rather than just miss. Quoting is safe
        // without escaping here: splitting on non-alphanumerics leaves no
        // `"` inside a term to escape.
        let term = if chars >= FTS_PREFIX_MIN_CHARS {
            let stem: String = lower.chars().take(FTS_STEM_PREFIX_CHARS).collect();
            format!("\"{stem}\"*")
        } else {
            format!("\"{lower}\"")
        };
        // Deduped on the emitted term, not the word it came from: two
        // inflections of one word (`уведомление`, `уведомления`) collapse to
        // the same stem, and repeating it would just make BM25 count the
        // same evidence twice.
        if seen.insert(term.clone()) {
            terms.push(term);
        }
    }

    if terms.is_empty() {
        return None;
    }
    Some(terms.join(" OR "))
}

/// True when any extracted token looks like camelCase/PascalCase (not just
/// a plain English word like `notifications`).
pub fn has_identifier_token(tokens: &[String]) -> bool {
    tokens.iter().any(|t| looks_camel_or_pascal(t))
}

/// English-ish stem for fuzzy matching: lowercase, strip a trailing `s`
/// when the word is long enough (`notifications` → `notification`).
pub fn english_stem(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    if lower.len() >= 5 && lower.ends_with('s') && !lower.ends_with("ss") {
        return lower[..lower.len() - 1].to_string();
    }
    lower
}

/// Whether a symbol name matches `token` exactly (case-insensitive) or by
/// stem containment (`notifications` ⊂ `CollectNotificationService`).
pub fn symbol_name_matches_token(symbol_name: &str, token: &str) -> MatchTightness {
    if symbol_name.eq_ignore_ascii_case(token) {
        return MatchTightness::Exact;
    }
    let stem = english_stem(token);
    if stem.len() < MIN_STEM_LEN {
        return MatchTightness::None;
    }
    let name_lower = symbol_name.to_ascii_lowercase();
    if name_lower.contains(&stem) {
        return MatchTightness::Stem;
    }
    MatchTightness::None
}

/// How tightly a token matched a symbol name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchTightness {
    None,
    /// Stem / substring containment (e.g. notification ⊂ NotificationService).
    Stem,
    /// Exact case-insensitive name equality.
    Exact,
}

/// Inputs for assembling `SemanticSearchMeta` without depending on
/// `ToolMatch` (keeps this module free of `ai_tools`).
#[derive(Debug, Clone, Copy)]
pub struct SearchMetaInput<'a> {
    pub match_count: usize,
    pub symbol_hits: u32,
    pub has_semantic: bool,
    pub only_lexical: bool,
    pub tiers_used: &'a [String],
    pub extracted_tokens: &'a [String],
}

/// Decide whether a search looks weak and which Russian hint (if any) to
/// surface to the model. Returns `(weak, hint)`.
///
/// Soft hints (`weak: false`, `hint: Some`) fire when results exist but the
/// query had no camelCase/PascalCase identifiers — the model should refine
/// next time without treating this search as a failure.
pub fn weak_search_hint(input: SearchMetaInput<'_>) -> (bool, Option<String>) {
    if input.match_count == 0 {
        return (
            true,
            Some(
                "Ничего не найдено. Добавьте английские имена методов/классов (camelCase) и повторите поиск."
                    .to_string(),
            ),
        );
    }
    if input.extracted_tokens.is_empty() {
        return (
            true,
            Some(
                "В запросе нет английских идентификаторов — добавьте имена из кода (getXxx, XxxService)."
                    .to_string(),
            ),
        );
    }
    if input.only_lexical && !input.has_semantic && input.symbol_hits == 0 {
        return (
            true,
            Some(
                "Поиск шёл по тексту без совпадений по именам. Уточните query английскими терминами или дождитесь синхронизации эмбеддингов."
                    .to_string(),
            ),
        );
    }
    // Soft suggestion: hits exist, but query lacked camelCase/PascalCase —
    // still useful to nudge the model toward getXxx / XxxService next time.
    if !has_identifier_token(input.extracted_tokens) {
        let _ = input.tiers_used;
        return (
            false,
            Some(
                "В query нет camelCase/PascalCase имён (getXxx, XxxService). Для следующего уточнения добавьте имя операции или класса из результатов."
                    .to_string(),
            ),
        );
    }
    let _ = input.tiers_used;
    (false, None)
}

/// Whether a path segment (filename or directory name) matches `token`
/// case-insensitively, on `/` and `.` boundaries — including English stem
/// containment (`notifications` ⊂ `CollectNotificationService.java`).
pub fn path_segment_matches(relative_path: &str, token: &str) -> bool {
    let token_lower = token.to_ascii_lowercase();
    let stem = english_stem(token);
    for segment in relative_path.split('/') {
        if segment.eq_ignore_ascii_case(token) {
            return true;
        }
        if let Some((file_stem, _)) = segment.rsplit_once('.') {
            if file_stem.eq_ignore_ascii_case(token) {
                return true;
            }
            if stem.len() >= MIN_STEM_LEN && file_stem.to_ascii_lowercase().contains(&stem) {
                return true;
            }
        }
        if token_lower.len() >= MIN_STEM_LEN {
            let seg_lower = segment.to_ascii_lowercase();
            if seg_lower.contains(&token_lower) || (stem.len() >= MIN_STEM_LEN && seg_lower.contains(&stem))
            {
                return true;
            }
        }
    }
    false
}

fn split_raw_segments(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in query.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.') {
            current.push(ch);
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn is_path_like(s: &str) -> bool {
    let has_sep = s.contains('/') || s.contains('.') || s.contains('-') || s.contains('_');
    has_sep && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

fn path_stem(s: &str) -> Option<&str> {
    let name = s.rsplit('/').next().unwrap_or(s);
    if let Some((stem, ext)) = name.rsplit_once('.') {
        if !stem.is_empty() && !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphabetic()) {
            return Some(stem);
        }
    }
    None
}

fn classify_and_push(seen: &mut HashSet<String>, scored: &mut Vec<(TokenKind, String)>, raw: &str) {
    if looks_camel_or_pascal(raw) {
        push_token(seen, scored, TokenKind::CamelOrPascal, raw);
        return;
    }
    if raw.chars().all(|c| c.is_ascii_alphanumeric()) && raw.len() >= MIN_PLAIN_TOKEN_LEN {
        push_token(seen, scored, TokenKind::Plain, raw);
    }
}

fn looks_camel_or_pascal(s: &str) -> bool {
    if s.len() < 2 || !s.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first.is_lowercase() {
        return chars.any(|c| c.is_uppercase());
    }
    if first.is_uppercase() {
        let rest: Vec<char> = chars.collect();
        if rest.is_empty() {
            return false;
        }
        // ALLCAPS acronyms like API/FNS are plain, not Pascal
        if rest.iter().all(|c| c.is_uppercase() || c.is_ascii_digit()) {
            return false;
        }
        return rest.iter().any(|c| c.is_uppercase()) || rest.iter().any(|c| c.is_lowercase());
    }
    false
}

fn push_token(
    seen: &mut HashSet<String>,
    scored: &mut Vec<(TokenKind, String)>,
    kind: TokenKind,
    token: &str,
) {
    let key = token.to_ascii_lowercase();
    if seen.insert(key) {
        scored.push((kind, token.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_finds_camel_and_plain_in_mixed_query() {
        let tokens = extract_search_tokens(
            "алгоритм формирования списка уведомлений для подачи notifications",
        );
        assert!(tokens.iter().any(|t| t.eq_ignore_ascii_case("notifications")));
        // RU→EN expansion from «уведомлен»
        assert!(tokens.iter().any(|t| t == "Notification"));
    }

    #[test]
    fn extract_ru_en_from_cyrillic_only() {
        let tokens = extract_search_tokens("алгоритм формирования списка уведомлений для подачи");
        assert!(tokens.iter().any(|t| t == "Notification"));
        assert!(!tokens.is_empty());
    }

    #[test]
    fn extract_finds_pascal_and_camel_identifiers() {
        let tokens =
            extract_search_tokens("CollectNotificationService getPatentNotifications алгоритм");
        assert!(tokens.iter().any(|t| t == "CollectNotificationService"));
        assert!(tokens.iter().any(|t| t == "getPatentNotifications"));
        let idx_collect = tokens.iter().position(|t| t == "CollectNotificationService").unwrap();
        let idx_get = tokens.iter().position(|t| t == "getPatentNotifications").unwrap();
        assert!(idx_collect < 2 && idx_get < 2);
    }

    #[test]
    fn extract_path_like_and_stem() {
        let tokens = extract_search_tokens("docs/getPatentNotifications.adoc");
        assert!(tokens.iter().any(|t| t.contains("getPatentNotifications")));
    }

    #[test]
    fn extract_dedupes_case_insensitively() {
        let tokens = extract_search_tokens("UserService userservice");
        assert_eq!(
            tokens.iter().filter(|t| t.eq_ignore_ascii_case("UserService")).count(),
            1
        );
    }

    #[test]
    fn path_segment_matches_stem_and_directory() {
        assert!(path_segment_matches(
            "src/docs/asciidoc/getPatentNotifications/getPatentNotifications.adoc",
            "getPatentNotifications"
        ));
        assert!(path_segment_matches(
            "src/main/java/CollectNotificationService.java",
            "notifications"
        ));
        assert!(!path_segment_matches("src/foo/Bar.java", "xyz"));
    }

    #[test]
    fn symbol_name_stem_match() {
        assert_eq!(
            symbol_name_matches_token("CollectNotificationService", "notifications"),
            MatchTightness::Stem
        );
        assert_eq!(
            symbol_name_matches_token("CollectNotificationService", "CollectNotificationService"),
            MatchTightness::Exact
        );
        assert_eq!(
            symbol_name_matches_token("UserService", "notifications"),
            MatchTightness::None
        );
    }

    #[test]
    fn english_stem_strips_plural_s() {
        assert_eq!(english_stem("notifications"), "notification");
        assert_eq!(english_stem("Notification"), "notification");
        assert_eq!(english_stem("class"), "class"); // ends with ss — keep
    }

    #[test]
    fn weak_hint_empty_matches() {
        let (weak, hint) = weak_search_hint(SearchMetaInput {
            match_count: 0,
            symbol_hits: 0,
            has_semantic: false,
            only_lexical: false,
            tiers_used: &["symbol".into()],
            extracted_tokens: &["Foo".into()],
        });
        assert!(weak);
        assert!(hint.unwrap().contains("Ничего не найдено"));
    }

    #[test]
    fn weak_hint_no_tokens() {
        let (weak, hint) = weak_search_hint(SearchMetaInput {
            match_count: 1,
            symbol_hits: 0,
            has_semantic: true,
            only_lexical: false,
            tiers_used: &["symbol".into(), "semantic".into()],
            extracted_tokens: &[],
        });
        assert!(weak);
        assert!(hint.unwrap().contains("нет английских"));
    }

    #[test]
    fn weak_hint_only_lexical() {
        let (weak, hint) = weak_search_hint(SearchMetaInput {
            match_count: 1,
            symbol_hits: 0,
            has_semantic: false,
            only_lexical: true,
            tiers_used: &["symbol".into(), "lexical".into()],
            extracted_tokens: &["notifications".into()],
        });
        assert!(weak);
        assert!(hint.unwrap().contains("без совпадений по именам"));
    }

    #[test]
    fn soft_hint_when_plain_tokens_only_but_hits_ok() {
        let (weak, hint) = weak_search_hint(SearchMetaInput {
            match_count: 3,
            symbol_hits: 1,
            has_semantic: true,
            only_lexical: false,
            tiers_used: &["symbol".into(), "semantic".into()],
            extracted_tokens: &["notifications".into()],
        });
        assert!(!weak);
        assert!(hint.unwrap().contains("camelCase"));
    }

    #[test]
    fn weak_hint_symbol_hits_ok_with_identifier() {
        let (weak, hint) = weak_search_hint(SearchMetaInput {
            match_count: 1,
            symbol_hits: 1,
            has_semantic: false,
            only_lexical: false,
            tiers_used: &["symbol".into()],
            extracted_tokens: &["UserService".into()],
        });
        assert!(!weak);
        assert!(hint.is_none());
    }

    #[test]
    fn fts5_query_keeps_the_cyrillic_that_extract_search_tokens_drops() {
        let query = "алгоритм формирования уведомлений notifications";

        // The premise of this tier existing at all: the symbol tier's
        // tokenizer emits nothing that came from the Russian words —
        // `Notification` here is the RU→EN expansion, not the query text.
        assert!(
            extract_search_tokens(query)
                .iter()
                .all(|t| t.is_ascii()),
            "{:?}",
            extract_search_tokens(query)
        );

        let fts = fts5_query(query).unwrap();
        assert!(fts.contains("\"алгори\"*"), "{fts}");
        assert!(fts.contains("\"формир\"*"), "{fts}");
        assert!(fts.contains("\"уведом\"*"), "{fts}");
        assert!(fts.contains("\"notifi\"*"), "{fts}");
    }

    /// The point of cutting a term before prefixing it: Russian inflects the
    /// end of a word, so the full word is not a prefix of its own other
    /// forms — `"уведомления"*` would never reach `уведомлений`.
    #[test]
    fn fts5_query_cuts_terms_to_a_stem_that_survives_inflection() {
        let stem = "\"уведом\"*";
        assert!(fts5_query("уведомления").unwrap().contains(stem));
        assert!(fts5_query("уведомлений").unwrap().contains(stem));
        assert!(fts5_query("УВЕДОМЛЕНИЕ").unwrap().contains(stem));
    }

    #[test]
    fn fts5_query_joins_with_or_so_a_long_question_still_matches() {
        let fts = fts5_query("где описан порядок подачи").unwrap();
        // Implicit AND would demand every word in one chunk, which for a
        // natural-language question means no results at all.
        assert!(fts.contains(" OR "), "{fts}");
        assert!(!fts.contains(" AND "), "{fts}");
    }

    #[test]
    fn fts5_query_prefixes_only_terms_long_enough_to_stay_selective() {
        let fts = fts5_query("для уведомления").unwrap();
        // Three characters: matched whole, or `для*` would hit half the corpus.
        assert!(fts.contains("\"для\"") && !fts.contains("\"для\"*"), "{fts}");
        assert!(fts.contains("\"уведом\"*"), "{fts}");
    }

    #[test]
    fn fts5_query_quotes_terms_that_would_otherwise_parse_as_operators() {
        let fts = fts5_query("stub OR NEAR 2fa").unwrap();
        assert!(fts.contains("\"or\""), "{fts}");
        assert!(fts.contains("\"near\"*"), "{fts}");
        // A bare leading digit is an FTS5 syntax error, not just a miss.
        assert!(fts.contains("\"2fa\""), "{fts}");
    }

    #[test]
    fn fts5_query_is_none_when_nothing_survives_tokenization() {
        assert!(fts5_query("").is_none());
        assert!(fts5_query("  — ?! ").is_none());
        // Single characters are punctuation-level noise.
        assert!(fts5_query("a и").is_none());
    }

    #[test]
    fn fts5_query_dedupes_and_caps_term_count() {
        let fts = fts5_query("отчёт Отчёт ОТЧЁТ").unwrap();
        assert_eq!(fts.matches("\"отчёт\"").count(), 1, "{fts}");

        // Three characters each, so every one is a distinct term rather
        // than collapsing into a shared stem.
        let long: String = (0..80).map(|i| format!("t{i:02} ")).collect();
        let capped = fts5_query(&long).unwrap();
        assert_eq!(capped.matches(" OR ").count(), MAX_FTS_TERMS - 1, "{capped}");
    }
}
