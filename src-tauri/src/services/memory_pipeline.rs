//! Post-turn memory pipeline: extract → policy → OptMem store.
//!
//! The agent loop never calls this. `commands::memory_pipeline` fires it
//! asynchronously after a chat turn is persisted.

use std::collections::HashSet;
use std::path::Path;

use crate::domain::memory_extract::{
    extractor_prompt, parse_extractor_output, reconcile_prompt, ExtractorOutput, MemoryExtractError,
    MemoryFactScope, ReconcileNeighbor, TurnTranscript,
};
use crate::domain::memory_policy::{
    apply_policy, neighbors_for_facts, ApprovedFact, MemoryEntrySnapshot, MemoryPolicyConfig,
    SimilarEntry,
};
use crate::services::agent_memory::{self, AgentMemoryError, MemoryScope};

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
