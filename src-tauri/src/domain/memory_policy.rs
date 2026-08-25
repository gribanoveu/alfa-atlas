//! Deterministic gate between extractor candidates and OptMem.
//!
//! The extractor LLM is not allowed to decide what is stored. This module
//! is pure: no I/O, no LLM. Callers load existing OptMem lines and pass
//! them in as `MemoryEntrySnapshot`.

use std::sync::LazyLock;

use regex::RegexBuilder;

use super::memory_extract::{ExtractedFact, ExtractorOutput, MemoryFactScope, MemoryFactType};
use super::optmem::DEFAULT_ENTRY_CHARS;

pub const DEFAULT_CONFIDENCE_THRESHOLD: f32 = 0.85;
pub const MIN_FACT_CHARS: usize = 10;
pub const MAX_FACT_CHARS: usize = 500;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    LowConfidence,
    DisallowedType,
    Sensitive,
    Ephemeral,
    TooShort,
    TooLong,
    Duplicate,
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

pub fn apply_policy(
    output: ExtractorOutput,
    existing: &[MemoryEntrySnapshot],
    config: &MemoryPolicyConfig,
) -> Vec<ApprovedFact> {
    let mut approved = Vec::new();
    // Dedup within this batch as well as against the store.
    let mut batch_normalized = Vec::new();
    for fact in output.facts {
        match evaluate(&fact, existing, config) {
            PolicyDecision::Skip { .. } => {}
            PolicyDecision::Accept { text, scope } => {
                let key = normalize(&text);
                if batch_normalized.iter().any(|n| n == &key) {
                    continue;
                }
                batch_normalized.push(key);
                approved.push(ApprovedFact {
                    text,
                    scope,
                    supersedes_id: None,
                });
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
                approved.push(ApprovedFact {
                    text,
                    scope,
                    supersedes_id: Some(old_id),
                });
            }
        }
    }
    approved
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

    PolicyDecision::Accept {
        text: flattened,
        scope: fact.scope,
    }
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
}
