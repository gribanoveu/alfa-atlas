//! Read-only introspection for `RepositoryIndex`/`ChunkIndex` — mirrors
//! `commands::workspace_index`'s shape (thin accessors straight over live
//! in-memory state, no walk, no I/O). Before this file existed, neither
//! index had any command of its own: both were only reachable indirectly,
//! rebuilt/read from inside `commands::embeddings::embedding_sync`, which
//! computed a per-language breakdown (`services::repo_index::RepoIndexStats`)
//! on every full sync and then discarded it.

use std::collections::HashMap;
use std::sync::Arc;

use tauri::State;

use crate::domain::repo_index::Language;
use crate::services::chunk_builder::ChunkIndex;
use crate::services::repo_index::RepositoryIndex;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoIndexSummary {
    pub files_indexed: usize,
    /// Keyed by a stable lowercase label (`language_label`), not
    /// `Language` itself — `Language` has no `Serialize` impl today
    /// (nothing has ever needed to send it over IPC), and adding one just
    /// to use it as a `HashMap` key risks the well-known serde_json
    /// non-string-key pitfall; converting explicitly here is simpler and
    /// avoids touching that enum for a single, local use.
    pub by_language: HashMap<String, usize>,
    pub chunks_indexed: usize,
}

fn language_label(language: Language) -> &'static str {
    match language {
        Language::Java => "java",
        Language::Json => "json",
        Language::Yaml => "yaml",
        Language::Markdown => "markdown",
        Language::AsciiDoc => "asciidoc",
    }
}

/// Live snapshot of `RepositoryIndex`/`ChunkIndex`'s current resident
/// state — computed by iterating already-in-memory data (no walk, no I/O),
/// so this is cheap to call any time. Deliberately does not require
/// `embedding_sync` to have run first — it just reports whatever the last
/// sync (or nothing, if none has run yet this session) left resident,
/// same "read whatever's there" contract `embedding_index_status` already
/// has for the embedding layer.
#[tauri::command]
pub fn repo_index_summary(
    repo_index: State<'_, Arc<RepositoryIndex>>,
    chunk_index: State<'_, Arc<ChunkIndex>>,
) -> Result<RepoIndexSummary, String> {
    let file_ids = repo_index.file_ids();
    let mut by_language: HashMap<String, usize> = HashMap::new();
    for id in &file_ids {
        if let Some(file) = repo_index.get(id) {
            *by_language.entry(language_label(file.metadata.language).to_string()).or_insert(0) += 1;
        }
    }
    Ok(RepoIndexSummary {
        files_indexed: file_ids.len(),
        by_language,
        chunks_indexed: chunk_index.chunk_ids().len(),
    })
}
