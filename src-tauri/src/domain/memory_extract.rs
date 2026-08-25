//! Candidate facts produced by the post-turn memory extractor LLM.
//!
//! The extractor is a tool-free one-shot call that sees only the latest
//! user/assistant turn. It does not decide what is stored — that is
//! `domain::memory_policy`. Persistence is OptMem (`services::agent_memory`).

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Which OptMem root a candidate fact belongs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryFactScope {
    Project,
    Global,
}

/// Kind of lasting fact. `Other` is accepted on the wire so a sloppy
/// extractor does not fail the whole JSON parse; policy then rejects it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryFactType {
    Preference,
    ProjectContext,
    TeamDecision,
    Tooling,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedFact {
    pub fact: String,
    #[serde(rename = "type")]
    pub fact_type: MemoryFactType,
    pub confidence: f32,
    pub scope: MemoryFactScope,
    #[serde(default)]
    pub supersedes_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ExtractorOutput {
    #[serde(default)]
    pub facts: Vec<ExtractedFact>,
}

/// The latest user/assistant pair the extractor is allowed to see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnTranscript {
    pub user_message: String,
    pub assistant_text: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MemoryExtractError {
    #[error("extractor returned no JSON object")]
    InvalidJson,
    #[error("turn has no extractable user/assistant pair")]
    EmptyTranscript,
}

/// Prompt for the tool-free extractor call. Instructs strict JSON, English
/// one-line facts, and OptMem-sized lines so policy rarely has to truncate.
pub fn extractor_prompt(turn: &TurnTranscript) -> String {
    format!(
        "You extract long-term memories from one assistant-chat turn in a documentation/code IDE.\n\
         \n\
         Return ONLY a JSON object of this exact shape (no markdown, no commentary):\n\
         {{\"facts\":[{{\"fact\":\"...\",\"type\":\"preference|project_context|team_decision|tooling\",\"confidence\":0.0,\"scope\":\"project|global\",\"supersedes_hint\":null}}]}}\n\
         \n\
         Rules:\n\
         - Usually return an empty facts array. That is the correct default.\n\
         - At most 2 facts. Never more.\n\
         - Extract only durable facts that will still matter in a later session: an explicit user preference, a team decision the user confirmed, or a lasting repository convention.\n\
         - Do NOT store Atlas/assistant capabilities (built-in skills such as rest-endpoint-docs or openapi-specs-layout, getAsciidocTemplates, integrity checks, harness tools).\n\
         - Do NOT store this turn's findings: broken/malformed tables, empty sections, typos, missing files, audit scores (56/97, 12 of 13 pass), checklists of issues.\n\
         - Do NOT store implementation details the assistant discovered by reading code and that can be re-read (@ValidMonths, a specific Gateway method).\n\
         - Architecture belongs in memory only if the USER confirmed it, not merely because the assistant found it in source.\n\
         - Skip secrets (passwords, API keys, tokens, credentials).\n\
         - Each fact is one dense English telegram-style line, 10–500 characters, no newlines.\n\
         - type must be one of: preference, project_context, team_decision, tooling.\n\
         - scope \"project\" = this repository (stack, docs structure, team decisions).\n\
         - scope \"global\" = user preferences that apply across projects.\n\
         - confidence is 0.0–1.0; use ≥ 0.90 only when the fact is explicit, not inferred.\n\
         - If this turn updates an earlier memory, set supersedes_hint to a distinctive substring of the old fact; otherwise null.\n\
         \n\
         USER:\n{}\n\
         \n\
         ASSISTANT:\n{}",
        turn.user_message, turn.assistant_text
    )
}

/// One already-stored OptMem line shown to the extractor on the reconcile
/// pass. Domain-pure display DTO — ranking lives in `memory_policy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileNeighbor {
    pub id: usize,
    pub text: String,
    pub scope: MemoryFactScope,
}

/// Second extractor call: drafts plus the nearest already-stored lines.
/// Does not include the original turn (the first call already saw it).
pub fn reconcile_prompt(drafts: &[ExtractedFact], neighbors: &[ReconcileNeighbor]) -> String {
    let mut drafts_block = String::new();
    for (i, d) in drafts.iter().enumerate() {
        let hint = d
            .supersedes_hint
            .as_deref()
            .map(str::trim)
            .filter(|h| !h.is_empty())
            .unwrap_or("null");
        drafts_block.push_str(&format!(
            "  {i}. type={} scope={} confidence={:.2} supersedes_hint={hint}\n     {}\n",
            fact_type_wire(d.fact_type),
            scope_wire(d.scope),
            d.confidence,
            d.fact
        ));
    }
    let mut existing_block = String::new();
    for n in neighbors {
        existing_block.push_str(&format!(
            "  #{} [{}] {}\n",
            n.id,
            scope_wire(n.scope),
            n.text
        ));
    }
    format!(
        "You already proposed new long-term memories from one assistant-chat turn. \
         Nearest existing memories (already stored) are listed below.\n\
         \n\
         Return ONLY a JSON object of this exact shape (no markdown, no commentary):\n\
         {{\"facts\":[{{\"fact\":\"...\",\"type\":\"preference|project_context|team_decision|tooling\",\"confidence\":0.0,\"scope\":\"project|global\",\"supersedes_hint\":null}}]}}\n\
         \n\
         Rules:\n\
         - If an existing line already covers a draft, drop that draft.\n\
         - If a draft updates an existing line, keep it and set supersedes_hint to a distinctive substring of that existing text.\n\
         - Keep only genuinely new durable facts. Empty facts array is correct when nothing new remains.\n\
         - At most 2 facts. Do not invent facts that were not in the drafts.\n\
         \n\
         DRAFTS:\n{drafts_block}\n\
         EXISTING:\n{existing_block}"
    )
}

fn fact_type_wire(t: MemoryFactType) -> &'static str {
    match t {
        MemoryFactType::Preference => "preference",
        MemoryFactType::ProjectContext => "project_context",
        MemoryFactType::TeamDecision => "team_decision",
        MemoryFactType::Tooling => "tooling",
        MemoryFactType::Other => "other",
    }
}

fn scope_wire(s: MemoryFactScope) -> &'static str {
    match s {
        MemoryFactScope::Project => "project",
        MemoryFactScope::Global => "global",
    }
}

/// Pull a JSON object out of a model reply that may wrap it in fences or
/// trailing prose. Domain-pure: no I/O.
pub fn parse_extractor_output(raw: &str) -> Result<ExtractorOutput, MemoryExtractError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(ExtractorOutput { facts: Vec::new() });
    }
    let json = strip_json_candidate(trimmed);
    serde_json::from_str::<ExtractorOutput>(json).map_err(|_| MemoryExtractError::InvalidJson)
}

fn strip_json_candidate(s: &str) -> &str {
    let unfenced = if let Some(rest) = s.strip_prefix("```json") {
        rest
    } else if let Some(rest) = s.strip_prefix("```") {
        rest
    } else {
        s
    };
    let unfenced = unfenced.trim();
    let unfenced = unfenced.strip_suffix("```").unwrap_or(unfenced).trim();
    match (unfenced.find('{'), unfenced.rfind('}')) {
        (Some(lo), Some(hi)) if hi >= lo => &unfenced[lo..=hi],
        _ => unfenced,
    }
}

/// Last user + last assistant text in `messages[after_ordinal+1..]`.
/// `messages` is the frontend `ChatMessage[]` JSON as stored by
/// `infra::chat_store` (opaque there; this pipeline is the one reader).
/// Returns `None` when that slice is empty. `last_ordinal` is always the
/// last index of `messages` so a watermark can advance even with no pair.
pub fn pending_turn(
    messages: &[serde_json::Value],
    after_ordinal: i64,
) -> Option<PendingTurn> {
    if messages.is_empty() {
        return None;
    }
    let last_ordinal = (messages.len() - 1) as i64;
    let start = after_ordinal.saturating_add(1).max(0) as usize;
    if start >= messages.len() {
        return None;
    }
    let mut user: Option<String> = None;
    let mut assistant: Option<String> = None;
    for msg in &messages[start..] {
        match msg.get("role").and_then(|r| r.as_str()) {
            Some("user") => {
                if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
                    let t = text.trim();
                    if !t.is_empty() {
                        user = Some(t.to_string());
                    }
                }
            }
            Some("assistant") => {
                let text = assistant_plain_text(msg);
                if !text.is_empty() {
                    assistant = Some(text);
                }
            }
            _ => {}
        }
    }
    let transcript = match (user, assistant) {
        (Some(user_message), Some(assistant_text)) => Some(TurnTranscript {
            user_message,
            assistant_text,
        }),
        _ => None,
    };
    Some(PendingTurn {
        transcript,
        last_ordinal,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTurn {
    pub transcript: Option<TurnTranscript>,
    pub last_ordinal: i64,
}

fn assistant_plain_text(msg: &serde_json::Value) -> String {
    if let Some(blocks) = msg.get("blocks").and_then(|b| b.as_array()) {
        let mut parts = Vec::new();
        for block in blocks {
            if block.get("type").and_then(|t| t.as_str()) != Some("text") {
                continue;
            }
            if let Some(content) = block.get("content").and_then(|c| c.as_str()) {
                let t = content.trim();
                if !t.is_empty() {
                    parts.push(t);
                }
            }
        }
        return parts.join("\n");
    }
    msg.get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_bare_json() {
        let out = parse_extractor_output(
            r#"{"facts":[{"fact":"User prefers Rust","type":"preference","confidence":0.9,"scope":"global"}]}"#,
        )
        .unwrap();
        assert_eq!(out.facts.len(), 1);
        assert_eq!(out.facts[0].fact_type, MemoryFactType::Preference);
        assert_eq!(out.facts[0].scope, MemoryFactScope::Global);
        assert!(out.facts[0].supersedes_hint.is_none());
    }

    #[test]
    fn parse_strips_markdown_fence_and_prose() {
        let raw = "Here you go:\n```json\n{\"facts\":[]}\n```\nThanks.";
        let out = parse_extractor_output(raw).unwrap();
        assert!(out.facts.is_empty());
    }

    #[test]
    fn parse_empty_string_is_zero_facts() {
        let out = parse_extractor_output("   ").unwrap();
        assert!(out.facts.is_empty());
    }

    #[test]
    fn parse_rejects_non_object() {
        assert_eq!(
            parse_extractor_output("not json"),
            Err(MemoryExtractError::InvalidJson)
        );
    }

    #[test]
    fn pending_turn_skips_already_extracted_prefix() {
        let messages = vec![
            serde_json::json!({"role":"user","content":"old"}),
            serde_json::json!({"role":"assistant","blocks":[{"type":"text","content":"old answer"}]}),
            serde_json::json!({"role":"user","content":"new q"}),
            serde_json::json!({
                "role":"assistant",
                "blocks":[
                    {"type":"reasoning","content":"think"},
                    {"type":"text","content":"new a"},
                    {"type":"toolCall","name":"readFile"}
                ]
            }),
        ];
        let pending = pending_turn(&messages, 1).unwrap();
        assert_eq!(pending.last_ordinal, 3);
        let t = pending.transcript.unwrap();
        assert_eq!(t.user_message, "new q");
        assert_eq!(t.assistant_text, "new a");
    }

    #[test]
    fn pending_turn_none_when_watermark_is_current() {
        let messages = vec![serde_json::json!({"role":"user","content":"hi"})];
        assert!(pending_turn(&messages, 0).is_none());
    }

    #[test]
    fn pending_turn_without_assistant_has_no_transcript_but_advances() {
        let messages = vec![serde_json::json!({"role":"user","content":"hi"})];
        let pending = pending_turn(&messages, -1).unwrap();
        assert!(pending.transcript.is_none());
        assert_eq!(pending.last_ordinal, 0);
    }

    #[test]
    fn extractor_prompt_asks_for_empty_default_and_at_most_two() {
        let turn = TurnTranscript {
            user_message: "hello".into(),
            assistant_text: "hi".into(),
        };
        let prompt = extractor_prompt(&turn);
        assert!(prompt.contains("extract long-term memories"));
        assert!(prompt.contains("Usually return an empty facts array"));
        assert!(prompt.contains("At most 2 facts"));
        assert!(prompt.contains("rest-endpoint-docs"));
        assert!(prompt.contains("USER:\nhello"));
        assert!(prompt.contains("ASSISTANT:\nhi"));
    }

    #[test]
    fn reconcile_prompt_lists_drafts_and_nearest_ids_not_the_turn() {
        let drafts = [ExtractedFact {
            fact: "saveAusnDetails is a Kafka consumer".into(),
            fact_type: MemoryFactType::ProjectContext,
            confidence: 0.94,
            scope: MemoryFactScope::Project,
            supersedes_hint: None,
        }];
        let neighbors = [ReconcileNeighbor {
            id: 10,
            text: "saveAusnDetails is a Kafka consumer for MARKED_TRANSACTIONS topic".into(),
            scope: MemoryFactScope::Project,
        }];
        let prompt = reconcile_prompt(&drafts, &neighbors);
        assert!(prompt.contains("Nearest existing memories"));
        assert!(prompt.contains("#10"));
        assert!(prompt.contains("MARKED_TRANSACTIONS"));
        assert!(prompt.contains("saveAusnDetails is a Kafka consumer"));
        assert!(!prompt.contains("USER:"));
        assert!(!prompt.contains("ASSISTANT:"));
    }
}
