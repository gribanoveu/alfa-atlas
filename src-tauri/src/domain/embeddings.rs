//! Types for the embedding layer built on top of the Chunk Index:
//! `Chunk -> Embedding -> vector index`. This module knows nothing about
//! `fastembed`, ONNX, `usearch`, or HTTP — those are `infra` concerns
//! implementing `EmbeddingProvider`/the vector store against these types.

use thiserror::Error;

/// A dense embedding vector. BGE-M3 (the local provider) produces 1024
/// dimensions; a remote provider may differ — `EmbeddingProvider::dimensions`
/// is how a caller finds out which, rather than assuming 1024 everywhere.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedding(pub Vec<f32>);

/// What `EmbeddingIndex` stores per chunk. `chunk_hash` — not the chunk's
/// text — is the staleness signal: `EmbeddingIndex::sync` re-embeds a chunk
/// only when this no longer matches the chunk's current
/// `ChunkMetadata::hash`.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingRecord {
    pub chunk_hash: blake3::Hash,
    pub vector: Embedding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EmbeddingProviderKind {
    Local,
    Remote,
}

impl Default for EmbeddingProviderKind {
    fn default() -> Self {
        Self::Local
    }
}

/// Persisted globally (`AppSettings.embedding`) — one provider choice across
/// every project, not per-repo. The remote API key is deliberately **not**
/// a field here: it goes through `infra::embedding_credentials_store`
/// (encrypted, mirrors how the SSH private key is stored), never through
/// plain `settings.json`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingProviderConfig {
    #[serde(default)]
    pub kind: EmbeddingProviderKind,
    #[serde(default)]
    pub remote_base_url: Option<String>,
    #[serde(default)]
    pub remote_model: Option<String>,
    /// `usearch`'s index needs a fixed dimension count at construction
    /// time, and unlike the local BGE-M3 provider (always 1024), a remote
    /// service's dimension count depends entirely on which model it's
    /// running — there's no way to discover it without either an extra API
    /// round-trip or the user stating it up front. Settings asks for this
    /// when Remote is selected; `None` falls back to
    /// `DEFAULT_REMOTE_DIMENSIONS` (OpenAI's `text-embedding-3-small`
    /// size, the most common default).
    #[serde(default)]
    pub remote_dimensions: Option<usize>,
}

/// Fallback when `EmbeddingProviderConfig.remote_dimensions` is unset.
pub const DEFAULT_REMOTE_DIMENSIONS: usize = 1536;

/// Local model download/readiness state. `Downloading` only carries a
/// meaningful `progress` if the download path used one of `fastembed`'s
/// coarse-grained hooks — see `services::embedding_model` for the current
/// caveat on how fine-grained this actually is.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ModelStatus {
    NotDownloaded,
    Downloading { progress: f32 },
    Ready,
    Error { message: String },
}

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("embedding provider error: {0}")]
    Provider(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("vector store error: {0}")]
    VectorStore(String),
    #[error("io error: {0}")]
    Io(#[source] std::io::Error),
    #[error("{0}")]
    Message(String),
}

/// One embedding backend — local on-device inference or a remote HTTP API,
/// selected by `EmbeddingProviderConfig.kind`. Synchronous (not `async fn`):
/// both concrete implementations are naturally blocking (`fastembed`'s
/// inference has no Tokio dependency; the remote provider uses a blocking
/// HTTP client rather than expanding this project's minimal `tokio`
/// features), so callers run this inside `spawn_blocking` — the same
/// pattern `services::standards::check_standards` and
/// `services::ai_tools::execute_tool` already use on the IPC boundary.
pub trait EmbeddingProvider: Send + Sync {
    /// Batched — callers embed every pending chunk in one call, not one
    /// call per chunk.
    fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError>;
    fn dimensions(&self) -> usize;
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStats {
    pub embedded: usize,
    pub skipped_unchanged: usize,
    pub removed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_to_local_with_no_remote_fields() {
        let config = EmbeddingProviderConfig::default();
        assert_eq!(config.kind, EmbeddingProviderKind::Local);
        assert_eq!(config.remote_base_url, None);
        assert_eq!(config.remote_dimensions, None);
    }

    #[test]
    fn deserializes_legacy_config_without_remote_dimensions() {
        let config: EmbeddingProviderConfig =
            serde_json::from_str(r#"{"kind":"remote","remoteBaseUrl":"https://x"}"#).unwrap();
        assert_eq!(config.kind, EmbeddingProviderKind::Remote);
        assert_eq!(config.remote_base_url.as_deref(), Some("https://x"));
        assert_eq!(config.remote_dimensions, None);
    }

    #[test]
    fn model_status_json_shape_is_tagged_by_status() {
        let json = serde_json::to_string(&ModelStatus::Downloading { progress: 0.5 }).unwrap();
        assert_eq!(json, r#"{"status":"downloading","progress":0.5}"#);
    }
}
