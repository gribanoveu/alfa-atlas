//! Deterministic gate between extractor candidates and OptMem.
//!
//! The extractor LLM is not allowed to decide what is stored. This module
//! is pure: no I/O, no LLM. Callers load existing OptMem lines and pass
//! them in as `MemoryEntrySnapshot`.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::RegexBuilder;

use super::memory_extract::{ExtractedFact, ExtractorOutput, MemoryFactScope, MemoryFactType};
use super::optmem::DEFAULT_ENTRY_CHARS;

pub const DEFAULT_CONFIDENCE_THRESHOLD: f32 = 0.9;
pub const MIN_FACT_CHARS: usize = 10;
pub const MAX_FACT_CHARS: usize = 500;
pub const MAX_FACTS_PER_TURN: usize = 2;
pub const SIMILAR_PER_DRAFT: usize = 2;
pub const SIMILAR_PROMPT_CAP: usize = 8;

/// Near-dup without a shared identifier.
const JACCARD_DUP: f32 = 0.5;
/// Near-dup when the two lines share a CamelCase / path / SCREAMING_SNAKE id.
/// Lower than `JACCARD_DUP` so paraphrases of the same entity still collapse
/// (`saveAusnDetails` Kafka #10 vs #14).
const JACCARD_DUP_WITH_ID: f32 = 0.2;
/// Surface a neighbor in the reconcile prompt.
const JACCARD_SHOW: f32 = 0.3;
const JACCARD_SHOW_WITH_ID: f32 = 0.2;

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryPolicyConfig {
    pub confidence_threshold: f32,
}

impl Default for MemoryPolicyConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: DEFAULT_CONFIDENCE_THRESHOLD,
        }
    }
}

impl MemoryPolicyConfig {
    pub fn from_threshold(threshold: f32) -> Self {
        Self {
            confidence_threshold: if threshold.is_finite() {
                threshold.clamp(0.0, 1.0)
            } else {
                DEFAULT_CONFIDENCE_THRESHOLD
            },
        }
    }
}

/// Existing raw OptMem line, used for dedup / supersede matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryEntrySnapshot {
    pub id: usize,
    pub text: String,
    pub scope: MemoryFactScope,
}

/// A stored line ranked as similar to an extractor draft.
#[derive(Debug, Clone, PartialEq)]
pub struct SimilarEntry {
    pub id: usize,
    pub text: String,
    pub scope: MemoryFactScope,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    LowConfidence,
    DisallowedType,
    Sensitive,
    Ephemeral,
    TooShort,
    TooLong,
    Duplicate,
    AssistantMeta,
    TransientWork,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Accept { text: String, scope: MemoryFactScope },
    Skip { reason: SkipReason },
    Supersede {
        old_id: usize,
        text: String,
        scope: MemoryFactScope,
    },
}

/// A fact that passed policy and is ready to append to OptMem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedFact {
    pub text: String,
    pub scope: MemoryFactScope,
    /// When set, this note replaces the meaning of `old_id` (append-only:
    /// OptMem never deletes the old raw line).
    pub supersedes_id: Option<usize>,
}

static SENSITIVE: LazyLock<regex::Regex> = LazyLock::new(|| {
    RegexBuilder::new(
        r"(?i)\b(password|passwd|api[\s_-]*key|secret|token|bearer|private[\s_-]*key|authorization|пароль)\b",
    )
    .build()
    .expect("static sensitive regex")
});

static EPHEMERAL: LazyLock<regex::Regex> = LazyLock::new(|| {
    RegexBuilder::new(
        r"(?i)\b(today|this session|this turn|currently debugging|right now|just now|сегодня|прямо сейчас|в этой сессии|в этом сеансе)\b",
    )
    .build()
    .expect("static ephemeral regex")
});

/// Atlas skills / harness tools — product capabilities, not repo facts.
static ASSISTANT_META: LazyLock<regex::Regex> = LazyLock::new(|| {
    RegexBuilder::new(
        r"(?i)(rest-endpoint-docs|openapi-specs-layout|getasciidoctemplates|built-in skill|semanticsearch|listfiles|readfile|writefile|editfile|createdirectory|deletedirectory|requestfullrepoaccess|requestmodeswitch|createplan|updateplan|readplan|updateplantodo)",
    )
    .build()
    .expect("static assistant-meta regex")
});

/// This-turn findings / audit snapshots. `TeamDecision` is exempt in `evaluate`.
static TRANSIENT_WORK: LazyLock<regex::Regex> = LazyLock::new(|| {
    RegexBuilder::new(
        r"(?i)(broken|malformed|empty section|empty validation|parseerror|typo|placeholder e:null|out of \d+ points|\d+/\d+|fails standard|\d+ of \d+|incomplete row|опечатка|пуст(ая|ые) секц|сломанн|missing request\.adoc|missing response\.adoc)",
    )
    .build()
    .expect("static transient-work regex")
});

static CAMEL_IDENT: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"[A-Za-z]*[a-z][A-Z][A-Za-z0-9]*")
        .expect("static camel ident regex")
});

static SCREAMING_SNAKE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\b[A-Z]{2,}(?:_[A-Z0-9]+)+\b")
        .expect("static screaming-snake regex")
});

static PATH_IDENT: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"[\w./-]+\.(?:adoc|java|json|yaml|yml|md)\b|[A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+")
        .expect("static path ident regex")
});

static TOKEN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"[A-Za-z0-9_]+").expect("static token regex")
});

pub fn apply_policy(
    output: ExtractorOutput,
    existing: &[MemoryEntrySnapshot],
    config: &MemoryPolicyConfig,
) -> Vec<ApprovedFact> {
    let mut ranked: Vec<(usize, f32, ApprovedFact)> = Vec::new();
    let mut batch_normalized = Vec::new();
    for (index, fact) in output.facts.into_iter().enumerate() {
        match evaluate(&fact, existing, config) {
            PolicyDecision::Skip { .. } => {}
            PolicyDecision::Accept { text, scope } => {
                let key = normalize(&text);
                if batch_normalized.iter().any(|n| n == &key) {
                    continue;
                }
                batch_normalized.push(key);
                ranked.push((
                    index,
                    fact.confidence,
                    ApprovedFact {
                        text,
                        scope,
                        supersedes_id: None,
                    },
                ));
            }
            PolicyDecision::Supersede {
                old_id,
                text,
                scope,
            } => {
                let key = normalize(&text);
                if batch_normalized.iter().any(|n| n == &key) {
                    continue;
                }
                batch_normalized.push(key);
                ranked.push((
                    index,
                    fact.confidence,
                    ApprovedFact {
                        text,
                        scope,
                        supersedes_id: Some(old_id),
                    },
                ));
            }
        }
    }
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    ranked.truncate(MAX_FACTS_PER_TURN);
    ranked.sort_by_key(|(index, _, _)| *index);
    ranked.into_iter().map(|(_, _, fact)| fact).collect()
}

pub fn evaluate(
    fact: &ExtractedFact,
    existing: &[MemoryEntrySnapshot],
    config: &MemoryPolicyConfig,
) -> PolicyDecision {
    if !fact.confidence.is_finite() || fact.confidence < config.confidence_threshold {
        return PolicyDecision::Skip {
            reason: SkipReason::LowConfidence,
        };
    }
    if !matches!(
        fact.fact_type,
        MemoryFactType::Preference
            | MemoryFactType::ProjectContext
            | MemoryFactType::TeamDecision
            | MemoryFactType::Tooling
    ) {
        return PolicyDecision::Skip {
            reason: SkipReason::DisallowedType,
        };
    }

    let flattened = flatten_one_line(&fact.fact);
    let char_len = flattened.chars().count();
    if char_len < MIN_FACT_CHARS {
        return PolicyDecision::Skip {
            reason: SkipReason::TooShort,
        };
    }
    if char_len > MAX_FACT_CHARS || flattened.len() > DEFAULT_ENTRY_CHARS {
        return PolicyDecision::Skip {
            reason: SkipReason::TooLong,
        };
    }
    if SENSITIVE.is_match(&flattened) {
        return PolicyDecision::Skip {
            reason: SkipReason::Sensitive,
        };
    }
    if EPHEMERAL.is_match(&flattened) {
        return PolicyDecision::Skip {
            reason: SkipReason::Ephemeral,
        };
    }
    if ASSISTANT_META.is_match(&flattened) {
        return PolicyDecision::Skip {
            reason: SkipReason::AssistantMeta,
        };
    }
    if fact.fact_type != MemoryFactType::TeamDecision && TRANSIENT_WORK.is_match(&flattened) {
        return PolicyDecision::Skip {
            reason: SkipReason::TransientWork,
        };
    }

    let same_scope: Vec<&MemoryEntrySnapshot> = existing
        .iter()
        .filter(|e| e.scope == fact.scope)
        .collect();

    let normalized = normalize(&flattened);
    if same_scope.iter().any(|e| normalize(&e.text) == normalized) {
        return PolicyDecision::Skip {
            reason: SkipReason::Duplicate,
        };
    }

    if let Some(hint) = fact
        .supersedes_hint
        .as_deref()
        .map(str::trim)
        .filter(|h| h.len() >= 3)
    {
        let hint_lc = hint.to_lowercase();
        if let Some(old) = same_scope
            .iter()
            .find(|e| e.text.to_lowercase().contains(&hint_lc))
        {
            return PolicyDecision::Supersede {
                old_id: old.id,
                text: format!("[updated] {flattened}"),
                scope: fact.scope,
            };
        }
    }

    if same_scope.iter().any(|e| is_near_duplicate(&flattened, &e.text)) {
        return PolicyDecision::Skip {
            reason: SkipReason::Duplicate,
        };
    }

    PolicyDecision::Accept {
        text: flattened,
        scope: fact.scope,
    }
}

/// Top-`k` already-stored lines in the same scope whose Jaccard (with an
/// identifier bonus) clears the show threshold.
pub fn similar_entries(
    candidate: &str,
    scope: MemoryFactScope,
    existing: &[MemoryEntrySnapshot],
    k: usize,
) -> Vec<SimilarEntry> {
    if k == 0 {
        return Vec::new();
    }
    let mut scored: Vec<SimilarEntry> = existing
        .iter()
        .filter(|e| e.scope == scope)
        .filter_map(|e| {
            let score = similarity_score(candidate, &e.text);
            if shows_as_neighbor(score, candidate, &e.text) {
                Some(SimilarEntry {
                    id: e.id,
                    text: e.text.clone(),
                    scope: e.scope,
                    score,
                })
            } else {
                None
            }
        })
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    });
    scored.truncate(k);
    scored
}

/// Union of per-draft neighbors, highest score first, capped for the prompt.
pub fn neighbors_for_facts(
    facts: &[ExtractedFact],
    existing: &[MemoryEntrySnapshot],
) -> Vec<SimilarEntry> {
    let mut by_id: HashMap<usize, SimilarEntry> = HashMap::new();
    for fact in facts {
        for hit in similar_entries(&fact.fact, fact.scope, existing, SIMILAR_PER_DRAFT) {
            by_id
                .entry(hit.id)
                .and_modify(|old| {
                    if hit.score > old.score {
                        *old = hit.clone();
                    }
                })
                .or_insert(hit);
        }
    }
    let mut out: Vec<SimilarEntry> = by_id.into_values().collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.id.cmp(&b.id))
    });
    out.truncate(SIMILAR_PROMPT_CAP);
    out
}

fn shows_as_neighbor(score: f32, a: &str, b: &str) -> bool {
    if score >= JACCARD_SHOW {
        return true;
    }
    score >= JACCARD_SHOW_WITH_ID && shares_identifier(a, b)
}

fn is_near_duplicate(a: &str, b: &str) -> bool {
    let score = similarity_score(a, b);
    if score >= JACCARD_DUP {
        return true;
    }
    score >= JACCARD_DUP_WITH_ID && shares_identifier(a, b)
}

fn similarity_score(a: &str, b: &str) -> f32 {
    let left = token_set(a);
    let right = token_set(b);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let inter = left.intersection(&right).count() as f32;
    let union = left.union(&right).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

fn shares_identifier(a: &str, b: &str) -> bool {
    let left = identifier_set(a);
    if left.is_empty() {
        return false;
    }
    identifier_set(b).intersection(&left).next().is_some()
}

fn token_set(s: &str) -> HashSet<String> {
    let stripped = strip_updated_prefix(s);
    TOKEN
        .find_iter(stripped)
        .map(|m| m.as_str())
        .filter(|t| t.chars().count() >= 4)
        .map(stem_token)
        .collect()
}

fn identifier_set(s: &str) -> HashSet<String> {
    let stripped = strip_updated_prefix(s);
    let mut out = HashSet::new();
    for m in CAMEL_IDENT.find_iter(stripped) {
        out.insert(m.as_str().to_lowercase());
    }
    for m in SCREAMING_SNAKE.find_iter(stripped) {
        out.insert(m.as_str().to_lowercase());
    }
    for m in PATH_IDENT.find_iter(stripped) {
        out.insert(m.as_str().to_lowercase());
    }
    out
}

fn stem_token(t: &str) -> String {
    let lower = t.to_lowercase();
    if lower.len() >= 5 && lower.ends_with('s') && !lower.ends_with("ss") {
        lower[..lower.len() - 1].to_string()
    } else {
        lower
    }
}

fn strip_updated_prefix(s: &str) -> &str {
    s.strip_prefix("[updated] ")
        .or_else(|| s.strip_prefix("[updated]"))
        .unwrap_or(s)
}

fn flatten_one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize(s: &str) -> String {
    flatten_one_line(s).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory_extract::ExtractedFact;

    fn fact(text: &str, confidence: f32) -> ExtractedFact {
        ExtractedFact {
            fact: text.to_string(),
            fact_type: MemoryFactType::Preference,
            confidence,
            scope: MemoryFactScope::Global,
            supersedes_hint: None,
        }
    }

    fn project_fact(text: &str, confidence: f32) -> ExtractedFact {
        let mut f = fact(text, confidence);
        f.fact_type = MemoryFactType::ProjectContext;
        f.scope = MemoryFactScope::Project;
        f
    }

    fn cfg() -> MemoryPolicyConfig {
        MemoryPolicyConfig::default()
    }

    #[test]
    fn rejects_low_confidence() {
        match evaluate(&fact("User prefers local embeddings", 0.5), &[], &cfg()) {
            PolicyDecision::Skip {
                reason: SkipReason::LowConfidence,
            } => {}
            other => panic!("expected skip, got {other:?}"),
        }
    }

    #[test]
    fn rejects_other_type() {
        let mut f = fact("User prefers local embeddings", 0.95);
        f.fact_type = MemoryFactType::Other;
        match evaluate(&f, &[], &cfg()) {
            PolicyDecision::Skip {
                reason: SkipReason::DisallowedType,
            } => {}
            other => panic!("expected skip, got {other:?}"),
        }
    }

    #[test]
    fn rejects_sensitive() {
        match evaluate(&fact("API key is sk-abc1234567", 0.99), &[], &cfg()) {
            PolicyDecision::Skip {
                reason: SkipReason::Sensitive,
            } => {}
            other => panic!("expected skip, got {other:?}"),
        }
    }

    #[test]
    fn rejects_ephemeral() {
        match evaluate(
            &fact("User is currently debugging the parser", 0.99),
            &[],
            &cfg(),
        ) {
            PolicyDecision::Skip {
                reason: SkipReason::Ephemeral,
            } => {}
            other => panic!("expected skip, got {other:?}"),
        }
    }

    #[test]
    fn rejects_too_short() {
        match evaluate(&fact("Uses Rust", 0.99), &[], &cfg()) {
            PolicyDecision::Skip {
                reason: SkipReason::TooShort,
            } => {}
            other => panic!("expected skip, got {other:?}"),
        }
    }

    #[test]
    fn rejects_duplicate_normalized() {
        let existing = [MemoryEntrySnapshot {
            id: 3,
            text: "User prefers local embeddings".to_string(),
            scope: MemoryFactScope::Global,
        }];
        match evaluate(&fact("  USER prefers   local embeddings  ", 0.99), &existing, &cfg())
        {
            PolicyDecision::Skip {
                reason: SkipReason::Duplicate,
            } => {}
            other => panic!("expected skip, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_does_not_cross_scope() {
        let existing = [MemoryEntrySnapshot {
            id: 3,
            text: "User prefers local embeddings".to_string(),
            scope: MemoryFactScope::Project,
        }];
        match evaluate(&fact("User prefers local embeddings", 0.99), &existing, &cfg()) {
            PolicyDecision::Accept { .. } => {}
            other => panic!("expected accept, got {other:?}"),
        }
    }

    #[test]
    fn supersede_writes_updated_line() {
        let existing = [MemoryEntrySnapshot {
            id: 7,
            text: "User uses Qdrant for vector search".to_string(),
            scope: MemoryFactScope::Project,
        }];
        let mut f = fact("User switched from Qdrant to usearch", 0.94);
        f.fact_type = MemoryFactType::Tooling;
        f.scope = MemoryFactScope::Project;
        f.supersedes_hint = Some("Qdrant".to_string());
        match evaluate(&f, &existing, &cfg()) {
            PolicyDecision::Supersede {
                old_id,
                text,
                scope,
            } => {
                assert_eq!(old_id, 7);
                assert_eq!(scope, MemoryFactScope::Project);
                assert!(text.starts_with("[updated] "));
                assert!(text.contains("usearch"));
            }
            other => panic!("expected supersede, got {other:?}"),
        }
    }

    #[test]
    fn apply_policy_drops_skips_and_batch_dupes() {
        let output = ExtractorOutput {
            facts: vec![
                fact("User prefers local embeddings", 0.4),
                fact("User prefers local embeddings", 0.96),
                fact("User prefers local embeddings", 0.97),
            ],
        };
        let approved = apply_policy(output, &[], &cfg());
        assert_eq!(approved.len(), 1);
        assert_eq!(approved[0].text, "User prefers local embeddings");
    }

    #[test]
    fn accepts_high_confidence_preference() {
        match evaluate(&fact("User prefers Rust for backend", 0.91), &[], &cfg()) {
            PolicyDecision::Accept { text, scope } => {
                assert_eq!(text, "User prefers Rust for backend");
                assert_eq!(scope, MemoryFactScope::Global);
            }
            other => panic!("expected accept, got {other:?}"),
        }
    }

    #[test]
    fn rejects_assistant_meta_skills_from_real_log() {
        let samples = [
            "Assistant has built-in skill 'rest-endpoint-docs' for filling REST method documentation folder after skeleton creation",
            "Assistant has built-in skill 'openapi-specs-layout' for multi-file OpenAPI spec structure of Atlas project",
            "Documentation uses corporate templates via getAsciidocTemplates and integrity checks for broken links, anchors, cyclic includes",
        ];
        for text in samples {
            match evaluate(&project_fact(text, 0.99), &[], &cfg()) {
                PolicyDecision::Skip {
                    reason: SkipReason::AssistantMeta,
                } => {}
                other => panic!("expected AssistantMeta for {text:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn rejects_transient_audit_findings_from_real_log() {
        let broken = "findAusnTransactionById-request.adoc has broken parameter table (parseError: incomplete row); columns for organizationId and transactionId incomplete.";
        match evaluate(&project_fact(broken, 0.99), &[], &cfg()) {
            PolicyDecision::Skip {
                reason: SkipReason::TransientWork,
            } => {}
            other => panic!("expected TransientWork for broken table, got {other:?}"),
        }
        let scores = "Documentation audit vs corporate standard K.1.1–K.7.1: no folder passed 80% threshold; best score is updateTransactionSpecifics with 56 out of 97 points";
        match evaluate(&project_fact(scores, 0.99), &[], &cfg()) {
            PolicyDecision::Skip {
                reason: SkipReason::TransientWork,
            } => {}
            other => panic!("expected TransientWork for audit scores, got {other:?}"),
        }
    }

    #[test]
    fn team_decision_placeholder_is_not_transient() {
        let mut f = project_fact(
            "Team decision: create placeholder request/response adocs for saveAusnDetails with explanatory NOTE, avoiding fake contract",
            0.99,
        );
        f.fact_type = MemoryFactType::TeamDecision;
        match evaluate(&f, &[], &cfg()) {
            PolicyDecision::Accept { .. } => {}
            other => panic!("expected accept for team decision, got {other:?}"),
        }
    }

    #[test]
    fn kafka_paraphrase_is_near_duplicate() {
        let existing = [MemoryEntrySnapshot {
            id: 10,
            text: "saveAusnDetails is a Kafka consumer for MARKED_TRANSACTIONS topic; no REST response exists, only async processing".into(),
            scope: MemoryFactScope::Project,
        }];
        let draft = project_fact(
            "saveAusnDetails methods are Kafka consumers with no response; request/response doc stubs contain explanatory placeholders",
            0.99,
        );
        match evaluate(&draft, &existing, &cfg()) {
            PolicyDecision::Skip {
                reason: SkipReason::Duplicate,
            } => {}
            other => panic!("expected Duplicate for Kafka paraphrase, got {other:?}"),
        }
    }

    #[test]
    fn similar_entries_ranks_kafka_original_first() {
        let existing = [
            MemoryEntrySnapshot {
                id: 10,
                text: "saveAusnDetails is a Kafka consumer for MARKED_TRANSACTIONS topic; no REST response exists, only async processing".into(),
                scope: MemoryFactScope::Project,
            },
            MemoryEntrySnapshot {
                id: 12,
                text: "Full repo root is './'; Java code paths start from src/main/java, not docs/asciidoc prefix".into(),
                scope: MemoryFactScope::Project,
            },
        ];
        let hits = similar_entries(
            "saveAusnDetails methods are Kafka consumers with no response; request/response doc stubs contain explanatory placeholders",
            MemoryFactScope::Project,
            &existing,
            2,
        );
        assert!(!hits.is_empty(), "expected a neighbor for the Kafka draft");
        assert_eq!(hits[0].id, 10);
        assert!(hits[0].score > 0.15);
    }

    #[test]
    fn apply_policy_caps_at_two_facts_per_turn() {
        let output = ExtractorOutput {
            facts: vec![
                fact("User prefers AsciiDoc over Markdown for docs", 0.95),
                fact("User prefers local embeddings for search", 0.97),
                fact("User prefers short commit messages always", 0.96),
                fact("User prefers dark theme in the editor", 0.94),
                fact("User prefers English for assistant replies", 0.93),
            ],
        };
        let approved = apply_policy(output, &[], &cfg());
        assert_eq!(approved.len(), 2);
        assert_eq!(approved[0].text, "User prefers local embeddings for search");
        assert_eq!(approved[1].text, "User prefers short commit messages always");
    }
}
