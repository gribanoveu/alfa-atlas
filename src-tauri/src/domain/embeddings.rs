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
/// `ChunkMetadata::hash`. Deliberately does **not** carry the vector itself
/// — that already lives in `VectorStore`/`usearch::Index`, the only place
/// anything actually reads it back (via `EmbeddingIndex::search`); keeping
/// a second copy here would just double the resident memory cost of every
/// embedded chunk for no functional benefit.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingRecord {
    pub chunk_hash: blake3::Hash,
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

/// One entry from the compiled-in system providers manifest's top-level
/// `embedding` section (see `infra::embedding_provider_manifest`) — a
/// global default, independent of any LLM provider id. Deserialize-only;
/// every field is `Option` so a fork can leave the template as explicit
/// `null`s (meaning "use the Local BGE-M3 provider") and fill them later
/// without touching Rust.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingPreset {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub dimensions: Option<usize>,
    #[serde(default)]
    pub trusted_cert_pem: Option<String>,
    /// Alfa internal API: required `systemId` header on `/embeddings`.
    #[serde(default)]
    pub system_id: Option<String>,
    /// Skip TLS certificate validation — sandbox-only escape hatch when the
    /// server leaf cert is expired but the endpoint is still reachable.
    #[serde(default)]
    pub disable_tls_verification: Option<bool>,
}

/// Persisted globally (`AppSettings.embedding`) as an **override** layer on
/// top of the bundled `EmbeddingPreset` — same "field wins when `Some`,
/// `None` means inherit" shape as `LlmProviderConfig`. The remote API key
/// is deliberately **not** a field here: it goes through
/// `infra::embedding_credentials_store` (encrypted), never through plain
/// `settings.json`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingProviderConfig {
    /// `None` = inherit from the bundled preset (Remote when the preset
    /// has both `baseUrl` and `model`, else Local). Explicit `Some(Local)` /
    /// `Some(Remote)` pins the choice regardless of the preset.
    #[serde(default)]
    pub kind: Option<EmbeddingProviderKind>,
    #[serde(default)]
    pub remote_base_url: Option<String>,
    #[serde(default)]
    pub remote_model: Option<String>,
    /// `usearch`'s index needs a fixed dimension count at construction
    /// time, and unlike the local BGE-M3 provider (always 1024), a remote
    /// service's dimension count depends entirely on which model it's
    /// running — there's no way to discover it without either an extra API
    /// round-trip or the user stating it up front. Settings asks for this
    /// when Remote is selected; `None` falls back to the preset, then to
    /// `DEFAULT_REMOTE_DIMENSIONS` (OpenAI's `text-embedding-3-small`
    /// size, the most common default).
    #[serde(default)]
    pub remote_dimensions: Option<usize>,
    #[serde(default)]
    pub remote_trusted_cert_pem: Option<String>,
    #[serde(default)]
    pub remote_system_id: Option<String>,
    #[serde(default)]
    pub remote_disable_tls_verification: Option<bool>,
}

/// Merged, ready-to-use view of the embedding provider — bundled preset
/// folded with the settings-layer override. What
/// `services::embedding_config::resolve_embedding_config` produces and what
/// runtime / IPC actually consume. Serialize-only.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedEmbeddingConfig {
    pub kind: EmbeddingProviderKind,
    pub remote_base_url: Option<String>,
    pub remote_model: Option<String>,
    pub remote_dimensions: Option<usize>,
    pub remote_trusted_cert_pem: Option<String>,
    pub remote_system_id: Option<String>,
    pub remote_disable_tls_verification: bool,
}

impl Default for ResolvedEmbeddingConfig {
    fn default() -> Self {
        Self {
            kind: EmbeddingProviderKind::Local,
            remote_base_url: None,
            remote_model: None,
            remote_dimensions: None,
            remote_trusted_cert_pem: None,
            remote_system_id: None,
            remote_disable_tls_verification: false,
        }
    }
}

/// Fallback when neither the override nor the preset sets dimensions.
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
    #[error("tls configuration error: {0}")]
    Tls(String),
    #[error("vector store error: {0}")]
    VectorStore(String),
    #[error("io error: {0}")]
    Io(#[source] std::io::Error),
    #[error("{0}")]
    Message(String),
}

/// One embedding backend — local on-device inference or a remote HTTP API,
/// selected by `ResolvedEmbeddingConfig.kind`. Synchronous (not `async fn`):
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

/// Read-only "is this project's index built" signal — derived from the
/// persisted/resident `EmbeddingIndex` itself (`embedded_count`), not from
/// whether a `sync()` happened to run earlier in this process's lifetime.
/// Lets the UI show real state on mount instead of resetting to "not yet
/// synced" every time a component remounts.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingIndexStatus {
    pub synced: bool,
    pub embedded_count: usize,
    /// The persisted index exists but predates a `CHUNK_VERSION`/
    /// `INDEX_VERSION` bump (or was built for a different `index_root`) —
    /// `services::index_store_ensure` detected the mismatch but, being a
    /// read-only attach, left the on-disk data untouched rather than
    /// wiping it. `synced` is `false` in this case too (nothing was
    /// loaded), but the reason is different from "never synced": a real
    /// `embedding_sync` will repair and rebuild it, not start from a blank
    /// slate.
    pub stale: bool,
    /// Files still queued for `run_background_backlog_sync` — `0` when
    /// nothing's deferred (the common case: `embedding_sync` folds a small
    /// non-doc change set inline rather than deferring it, see
    /// `commands::embeddings::split_sync_tiers`). Non-zero after a fresh
    /// project's first sync, or after a routine sync catches a large
    /// upstream change (e.g. a big `git pull`) — either way this can be
    /// nonzero on more than just a project's very first sync now that
    /// documentation is prioritized on every sync, not just the first.
    /// Read from `commands::embeddings::BackgroundBacklogSlot`, not a
    /// hand-maintained counter, so it survives an app restart or a panicked
    /// background task without drifting.
    pub background_pending: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_to_inherit_with_no_remote_fields() {
        let config = EmbeddingProviderConfig::default();
        assert_eq!(config.kind, None);
        assert_eq!(config.remote_base_url, None);
        assert_eq!(config.remote_dimensions, None);
        assert_eq!(config.remote_trusted_cert_pem, None);
    }

    #[test]
    fn deserializes_legacy_config_kind_as_explicit_override() {
        let config: EmbeddingProviderConfig =
            serde_json::from_str(r#"{"kind":"remote","remoteBaseUrl":"https://x"}"#).unwrap();
        assert_eq!(config.kind, Some(EmbeddingProviderKind::Remote));
        assert_eq!(config.remote_base_url.as_deref(), Some("https://x"));
        assert_eq!(config.remote_dimensions, None);
    }

    #[test]
    fn deserializes_legacy_config_without_kind_as_inherit() {
        let config: EmbeddingProviderConfig =
            serde_json::from_str(r#"{"remoteBaseUrl":"https://x"}"#).unwrap();
        assert_eq!(config.kind, None);
        assert_eq!(config.remote_base_url.as_deref(), Some("https://x"));
    }

    #[test]
    fn preset_deserializes_explicit_nulls() {
        let preset: EmbeddingPreset = serde_json::from_str(
            r#"{"baseUrl":null,"model":null,"dimensions":null,"trustedCertPem":null}"#,
        )
        .unwrap();
        assert_eq!(preset.base_url, None);
        assert_eq!(preset.model, None);
        assert_eq!(preset.dimensions, None);
        assert_eq!(preset.trusted_cert_pem, None);
    }

    #[test]
    fn model_status_json_shape_is_tagged_by_status() {
        let json = serde_json::to_string(&ModelStatus::Downloading { progress: 0.5 }).unwrap();
        assert_eq!(json, r#"{"status":"downloading","progress":0.5}"#);
    }
}
