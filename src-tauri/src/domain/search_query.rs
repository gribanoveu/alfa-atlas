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

/// Weight for a token in the lexical tier: camelCase/PascalCase × 2, else × 1.
pub fn lexical_token_weight(token: &str) -> f32 {
    if looks_camel_or_pascal(token) {
        2.0
    } else {
        1.0
    }
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
    fn lexical_weight_camel_is_double() {
        assert_eq!(lexical_token_weight("getName"), 2.0);
        assert_eq!(lexical_token_weight("UserService"), 2.0);
        assert_eq!(lexical_token_weight("notifications"), 1.0);
    }
}
