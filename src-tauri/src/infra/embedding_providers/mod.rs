pub mod local;
pub mod remote;

use crate::domain::embeddings::{
    EmbeddingError, EmbeddingProvider, EmbeddingProviderKind, ResolvedEmbeddingConfig,
    DEFAULT_REMOTE_DIMENSIONS,
};

/// Resolves a merged `ResolvedEmbeddingConfig` (and, for `Remote`, an API
/// key already read from `embedding_credentials_store`) into a concrete
/// `EmbeddingProvider`. The one place that decision is made — callers work
/// against the trait afterward, never against
/// `LocalEmbeddingProvider`/`RemoteEmbeddingProvider` directly.
pub fn provider_for(
    config: &ResolvedEmbeddingConfig,
    remote_api_key: Option<String>,
) -> Result<Box<dyn EmbeddingProvider>, EmbeddingError> {
    match config.kind {
        EmbeddingProviderKind::Local => Ok(Box::new(local::LocalEmbeddingProvider::try_new()?)),
        EmbeddingProviderKind::Remote => {
            let base_url = config.remote_base_url.clone().ok_or_else(|| {
                EmbeddingError::Message("remote provider selected without a base URL".into())
            })?;
            let model = config.remote_model.clone().ok_or_else(|| {
                EmbeddingError::Message("remote provider selected without a model name".into())
            })?;
            let api_key = remote_api_key.ok_or_else(|| {
                EmbeddingError::Message("remote provider selected without an API key".into())
            })?;
            let dimensions = config.remote_dimensions.unwrap_or(DEFAULT_REMOTE_DIMENSIONS);
            Ok(Box::new(remote::RemoteEmbeddingProvider::new(
                base_url,
                model,
                api_key,
                dimensions,
                config.remote_trusted_cert_pem.as_deref(),
                config.remote_system_id.clone(),
                config.remote_disable_tls_verification,
            )?))
        }
    }
}

/// The dimension count `provider_for` would produce for `config`, without
/// constructing the provider — for `Local` that means not loading the
/// ~570MB ONNX model just to read a compile-time constant. Callers that
/// only need to know "would a persisted index at dimension N still be
/// usable" (e.g. a read-only status check) should use this instead of
/// `provider_for(...).dimensions()`; callers that are about to actually
/// call `embed()` need the real provider anyway and should keep using
/// `provider_for`.
pub fn expected_dimensions(config: &ResolvedEmbeddingConfig) -> usize {
    match config.kind {
        EmbeddingProviderKind::Local => local::DIMENSIONS,
        EmbeddingProviderKind::Remote => {
            config.remote_dimensions.unwrap_or(DEFAULT_REMOTE_DIMENSIONS)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote_resolved() -> ResolvedEmbeddingConfig {
        ResolvedEmbeddingConfig {
            kind: EmbeddingProviderKind::Remote,
            remote_base_url: Some("https://api.example.com".to_string()),
            remote_model: Some("text-embedding-3-small".to_string()),
            remote_dimensions: None,
            remote_trusted_cert_pem: None,
            remote_system_id: None,
            remote_disable_tls_verification: false,
        }
    }

    #[test]
    fn remote_without_base_url_errors_clearly() {
        let config = ResolvedEmbeddingConfig {
            kind: EmbeddingProviderKind::Remote,
            remote_base_url: None,
            remote_model: Some("m".into()),
            remote_dimensions: None,
            remote_trusted_cert_pem: None,
            remote_system_id: None,
            remote_disable_tls_verification: false,
        };
        let Err(err) = provider_for(&config, Some("key".to_string())) else {
            panic!("expected an error");
        };
        assert!(matches!(err, EmbeddingError::Message(_)));
    }

    #[test]
    fn remote_without_api_key_errors_clearly() {
        let Err(err) = provider_for(&remote_resolved(), None) else {
            panic!("expected an error");
        };
        assert!(matches!(err, EmbeddingError::Message(_)));
    }

    #[test]
    fn expected_dimensions_uses_default_for_remote_without_pin() {
        assert_eq!(expected_dimensions(&remote_resolved()), DEFAULT_REMOTE_DIMENSIONS);
    }
}
