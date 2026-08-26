//! Post-turn memory pipeline: extract → policy → OptMem store.
//!
//! The agent loop never calls this. `commands::memory_pipeline` fires it
//! asynchronously after a chat turn is persisted.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::domain::memory_extract::{
    extractor_prompt, parse_extractor_output, reconcile_prompt, ExtractorOutput, MemoryExtractError,
    MemoryFactScope, ReconcileNeighbor, TurnTranscript,
};
use crate::domain::memory_extract::pending_turn;
use crate::domain::memory_policy::{
    apply_policy, neighbors_for_facts, ApprovedFact, MemoryEntrySnapshot, MemoryPolicyConfig,
    SimilarEntry,
};
use crate::domain::llm::{ChatEvent, ChatRequest, LlmMessage, LlmRole};

use crate::infra::{chat_store, llm_debug_log};
use crate::services::agent_memory::{self, AgentMemoryError, MemoryScope};
use crate::services::llm_chat::ChatEventSink;
use crate::services::llm_session::LlmProviderSlot;
use crate::services::{llm_config, llm_rate_limit, llm_session};

#[derive(Debug, thiserror::Error)]
pub enum MemoryPipelineError {
    #[error("{0}")]
    Extract(#[from] MemoryExtractError),
    #[error("{0}")]
    Memory(#[from] AgentMemoryError),
    #[error("{0}")]
    Llm(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreReport {
    pub noted: usize,
    pub naps: usize,
}

/// Ask the extractor LLM for candidate facts, then parse its JSON.
pub fn extract_from_turn<F>(
    transcript: &TurnTranscript,
    llm: F,
) -> Result<ExtractorOutput, MemoryPipelineError>
where
    F: FnOnce(&str) -> Result<String, String>,
{
    if transcript.user_message.trim().is_empty() || transcript.assistant_text.trim().is_empty() {
        return Err(MemoryPipelineError::Extract(MemoryExtractError::EmptyTranscript));
    }
    let prompt = extractor_prompt(transcript);
    let raw = llm(&prompt).map_err(MemoryPipelineError::Llm)?;
    Ok(parse_extractor_output(&raw)?)
}

pub fn load_existing_snapshots(
    repo_root: &Path,
) -> Result<Vec<MemoryEntrySnapshot>, MemoryPipelineError> {
    let mut out = Vec::new();
    for (scope, fact_scope) in [
        (MemoryScope::Project, MemoryFactScope::Project),
        (MemoryScope::Global, MemoryFactScope::Global),
    ] {
        for entry in agent_memory::list_raw_entries(scope, repo_root)? {
            out.push(MemoryEntrySnapshot {
                id: entry.id,
                text: entry.text,
                scope: fact_scope,
            });
        }
    }
    Ok(out)
}

pub fn facts_to_store(
    output: ExtractorOutput,
    existing: &[MemoryEntrySnapshot],
    config: &MemoryPolicyConfig,
) -> Vec<ApprovedFact> {
    apply_policy(output, existing, config)
}

fn to_mem_scope(scope: MemoryFactScope) -> MemoryScope {
    match scope {
        MemoryFactScope::Project => MemoryScope::Project,
        MemoryFactScope::Global => MemoryScope::Global,
    }
}

/// Append approved facts to OptMem, then drain pending TREE naps for every
/// scope that received a write.
pub fn store_facts<F>(
    facts: &[ApprovedFact],
    repo_root: &Path,
    mut llm_nap: F,
) -> Result<StoreReport, MemoryPipelineError>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let mut noted = 0usize;
    let mut touched: HashSet<MemoryScope> = HashSet::new();
    for fact in facts {
        agent_memory::note(to_mem_scope(fact.scope), repo_root, &fact.text)?;
        noted += 1;
        touched.insert(to_mem_scope(fact.scope));
    }
    let mut naps = 0usize;
    for scope in touched {
        naps += agent_memory::drain_pending_naps(scope, repo_root, &mut llm_nap)?;
    }
    Ok(StoreReport { noted, naps })
}

/// Full extract → nearest-neighbor reconcile → policy → store.
/// `llm` is used for the extractor, optional reconcile, and OptMem naps.
/// A reconcile LLM failure is an error — drafts are not stored, so the
/// watermark in `commands::memory_pipeline` does not advance.
pub fn run_turn<F>(
    transcript: &TurnTranscript,
    repo_root: &Path,
    config: &MemoryPolicyConfig,
    mut llm: F,
) -> Result<StoreReport, MemoryPipelineError>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let mut output = extract_from_turn(transcript, |prompt| llm(prompt))?;
    let existing = load_existing_snapshots(repo_root)?;
    let neighbors = neighbors_for_facts(&output.facts, &existing);
    if !neighbors.is_empty() {
        output = reconcile_with_neighbors(&output, &neighbors, |prompt| llm(prompt))?;
    }
    let approved = facts_to_store(output, &existing, config);
    store_facts(&approved, repo_root, llm)
}

fn reconcile_with_neighbors<F>(
    drafts: &ExtractorOutput,
    neighbors: &[SimilarEntry],
    llm: F,
) -> Result<ExtractorOutput, MemoryPipelineError>
where
    F: FnOnce(&str) -> Result<String, String>,
{
    let shown: Vec<ReconcileNeighbor> = neighbors
        .iter()
        .map(|n| ReconcileNeighbor {
            id: n.id,
            text: n.text.clone(),
            scope: n.scope,
        })
        .collect();
    let prompt = reconcile_prompt(&drafts.facts, &shown);
    let raw = llm(&prompt).map_err(MemoryPipelineError::Llm)?;
    Ok(parse_extractor_output(&raw)?)
}

/// Per-chat in-flight + dirty flag so a save that lands while a pass is
/// running is not dropped: the running job loops until the dirty bit is
/// clear.
pub struct MemoryExtractGuard {
    inner: Mutex<GuardInner>,
}

struct GuardInner {
    in_flight: HashSet<String>,
    dirty: HashSet<String>,
}

impl MemoryExtractGuard {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(GuardInner {
                in_flight: HashSet::new(),
                dirty: HashSet::new(),
            }),
        }
    }

    /// `true` = this caller should run the job. `false` = already running;
    /// the current pass will re-check after it finishes.
    pub fn try_start(&self, chat_id: &str) -> bool {
        let Ok(mut g) = self.inner.lock() else {
            return false;
        };
        if g.in_flight.contains(chat_id) {
            g.dirty.insert(chat_id.to_string());
            false
        } else {
            g.in_flight.insert(chat_id.to_string());
            g.dirty.remove(chat_id);
            true
        }
    }

    /// After one pass: `true` if another pass is needed.
    pub fn should_rerun(&self, chat_id: &str) -> bool {
        let Ok(mut g) = self.inner.lock() else {
            return false;
        };
        if g.dirty.remove(chat_id) {
            true
        } else {
            g.in_flight.remove(chat_id);
            false
        }
    }
}


/// One extraction pass over whatever of `chat_id` is not yet extracted.
///
/// Reports through `events` rather than emitting directly — the only thing
/// it reports is `ChatEvent::RateLimitChanged`, after the extraction call's
/// token usage is recorded. That keeps this module free of `tauri::` and
/// removes the last reason `commands::memory_pipeline` had to import from
/// `commands::llm`.
pub fn run_pending_pass(
    events: &ChatEventSink,
    chat_id: &str,
    repo_root: &str,
    slot: &LlmProviderSlot,
) -> Result<(), String> {
    let settings = llm_config::load_llm_settings().map_err(|e| e.to_string())?;
    if !settings.memory_extraction_enabled {
        return Ok(());
    }
    let stored_root = chat_store::chat_repo_root(chat_id).map_err(|e| e.to_string())?;
    if stored_root != repo_root {
        return Err(format!(
            "chat {chat_id} belongs to {stored_root}, not {repo_root}"
        ));
    }

    let watermark = chat_store::memory_extracted_ordinal(chat_id).map_err(|e| e.to_string())?;
    let loaded = chat_store::load_chat(chat_id).map_err(|e| e.to_string())?;
    let Some(pending) = pending_turn(&loaded.messages, watermark) else {
        return Ok(());
    };

    if let Some(transcript) = pending.transcript {
        let Some(provider_id) = llm_config::effective_active_provider_id(&settings) else {
            // No resolvable provider — leave the watermark so a later save retries.
            return Ok(());
        };

        let llm_session::LlmSession { provider, model, .. } =
            llm_session::resolve(&provider_id, slot)?;
        let debug = settings.debug_logging;
        let events = events.clone();
        let provider_id_for_log = provider_id.clone();

        let mut llm = |prompt: &str| -> Result<String, String> {
            let request = ChatRequest {
                messages: vec![LlmMessage {
                    role: LlmRole::User,
                    content: Some(prompt.to_string()),
                    tool_call_id: None,
                    tool_calls: vec![],
                }],
                tools: Vec::new(),
                model: model.clone(),
            };
            llm_debug_log::log_request(debug, &provider_id_for_log, llm_debug_log::ONCE_ROUND, &request);
            let outcome = provider.chat(request).map_err(|e| e.to_string());
            llm_debug_log::log_chat_once_result(debug, &provider_id_for_log, &outcome);
            if let Ok(ref response) = outcome {
                if let Some(usage) = response.usage {
                    llm_rate_limit::record(&provider_id_for_log, usage.completion_tokens);
                    events(ChatEvent::RateLimitChanged);
                }
            }
            outcome.map(|resp| resp.content.unwrap_or_default())
        };

        let config = MemoryPolicyConfig::from_threshold(settings.memory_confidence_threshold);
        let root = PathBuf::from(repo_root);
        // LLM failure must not advance the watermark — the next save retries.
        run_turn(&transcript, &root, &config, &mut llm)
            .map_err(|e| e.to_string())?;
    }

    chat_store::set_memory_extracted_ordinal(chat_id, pending.last_ordinal)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("memory-pipeline-repo-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn extract_from_turn_parses_mock_llm_json() {
        let turn = TurnTranscript {
            user_message: "I prefer Rust for backend work".into(),
            assistant_text: "Noted, we'll use Rust.".into(),
        };
        let out = extract_from_turn(&turn, |_prompt| {
            Ok(r#"{"facts":[{"fact":"User prefers Rust for backend work","type":"preference","confidence":0.94,"scope":"global"}]}"#.into())
        })
        .unwrap();
        assert_eq!(out.facts.len(), 1);
        assert_eq!(out.facts[0].fact, "User prefers Rust for backend work");
    }

    #[test]
    fn run_turn_notes_and_drains_via_mock_llm() {
        with_temp_home(|| {
            let repo = temp_repo();
            let turn = TurnTranscript {
                user_message: "I prefer Rust for backend work".into(),
                assistant_text: "We'll stick with Rust for backend components.".into(),
            };
            let report = run_turn(
                &turn,
                &repo,
                &MemoryPolicyConfig::default(),
                |prompt| {
                    if prompt.contains("extract long-term memories") {
                        Ok(r#"{"facts":[{"fact":"User prefers Rust for backend work","type":"preference","confidence":0.94,"scope":"project"}]}"#.into())
                    } else {
                        Ok("user prefers rust".into())
                    }
                },
            )
            .unwrap();
            assert_eq!(report.noted, 1);
            let entries = agent_memory::list_raw_entries(MemoryScope::Project, &repo).unwrap();
            assert_eq!(entries.len(), 1);
            assert!(entries[0].text.contains("Rust"));
            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn run_turn_skips_low_confidence_facts() {
        with_temp_home(|| {
            let repo = temp_repo();
            let turn = TurnTranscript {
                user_message: "hello".into(),
                assistant_text: "hi there friend".into(),
            };
            let report = run_turn(
                &turn,
                &repo,
                &MemoryPolicyConfig::default(),
                |_prompt| {
                    Ok(r#"{"facts":[{"fact":"User prefers Rust for backend work","type":"preference","confidence":0.2,"scope":"global"}]}"#.into())
                },
            )
            .unwrap();
            assert_eq!(report.noted, 0);
            assert!(agent_memory::list_raw_entries(MemoryScope::Global, &repo)
                .unwrap()
                .is_empty());
            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn run_turn_skips_reconcile_when_no_similar_hits() {
        with_temp_home(|| {
            let repo = temp_repo();
            let turn = TurnTranscript {
                user_message: "I prefer Rust for backend work".into(),
                assistant_text: "We'll stick with Rust.".into(),
            };
            let extract_calls = std::cell::Cell::new(0usize);
            let reconcile_calls = std::cell::Cell::new(0usize);
            let report = run_turn(
                &turn,
                &repo,
                &MemoryPolicyConfig::default(),
                |prompt| {
                    if prompt.contains("Nearest existing memories") {
                        reconcile_calls.set(reconcile_calls.get() + 1);
                        return Ok(r#"{"facts":[]}"#.into());
                    }
                    if prompt.contains("extract long-term memories") {
                        extract_calls.set(extract_calls.get() + 1);
                        return Ok(r#"{"facts":[{"fact":"User prefers Rust for backend work","type":"preference","confidence":0.94,"scope":"project"}]}"#.into());
                    }
                    Ok("user prefers rust".into())
                },
            )
            .unwrap();
            assert_eq!(extract_calls.get(), 1);
            assert_eq!(reconcile_calls.get(), 0);
            assert_eq!(report.noted, 1);
            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn run_turn_reconciles_near_match_and_empty_reply_stores_nothing() {
        with_temp_home(|| {
            let repo = temp_repo();
            agent_memory::note(
                MemoryScope::Project,
                &repo,
                "saveAusnDetails is a Kafka consumer for MARKED_TRANSACTIONS topic; no REST response exists, only async processing",
            )
            .unwrap();
            let turn = TurnTranscript {
                user_message: "how does saveAusnDetails work?".into(),
                assistant_text: "It is a Kafka consumer with no REST response.".into(),
            };
            let reconcile_calls = std::cell::Cell::new(0usize);
            let report = run_turn(
                &turn,
                &repo,
                &MemoryPolicyConfig::default(),
                |prompt| {
                    if prompt.contains("Nearest existing memories") {
                        reconcile_calls.set(reconcile_calls.get() + 1);
                        assert!(prompt.contains("#0") || prompt.contains("MARKED_TRANSACTIONS"));
                        return Ok(r#"{"facts":[]}"#.into());
                    }
                    if prompt.contains("extract long-term memories") {
                        return Ok(r#"{"facts":[{"fact":"saveAusnDetails methods are Kafka consumers with no response; request/response doc stubs contain explanatory placeholders","type":"project_context","confidence":0.96,"scope":"project"}]}"#.into());
                    }
                    Ok("summary".into())
                },
            )
            .unwrap();
            assert_eq!(reconcile_calls.get(), 1);
            assert_eq!(report.noted, 0);
            let entries = agent_memory::list_raw_entries(MemoryScope::Project, &repo).unwrap();
            assert_eq!(entries.len(), 1);
            fs::remove_dir_all(&repo).ok();
        });
    }
}
