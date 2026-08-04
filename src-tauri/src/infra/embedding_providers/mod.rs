pub mod local;
pub mod remote;

use crate::domain::embeddings::{
    EmbeddingError, EmbeddingProvider, EmbeddingProviderConfig, EmbeddingProviderKind,
    DEFAULT_REMOTE_DIMENSIONS,
};

/// Resolves a project's persisted `EmbeddingProviderConfig` (and, for
/// `Remote`, an API key already read from `embedding_credentials_store`)
/// into a concrete `EmbeddingProvider`. The one place that decision is
/// made — callers work against the trait afterward, never against
/// `LocalEmbeddingProvider`/`RemoteEmbeddingProvider` directly.
pub fn provider_for(
    config: &EmbeddingProviderConfig,
    remote_api_key: Option<String>,
) -> Result<Box<dyn EmbeddingProvider>, EmbeddingError> {
    match config.kind {
        EmbeddingProviderKind::Local => {
            Ok(Box::new(local::LocalEmbeddingProvider::try_new()?))
        }
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
                base_url, model, api_key, dimensions,
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_without_base_url_errors_clearly() {
        let config = EmbeddingProviderConfig {
            kind: EmbeddingProviderKind::Remote,
            ..Default::default()
        };
        let Err(err) = provider_for(&config, Some("key".to_string())) else {
            panic!("expected an error");
        };
        assert!(matches!(err, EmbeddingError::Message(_)));
    }

    #[test]
    fn remote_without_api_key_errors_clearly() {
        let config = EmbeddingProviderConfig {
            kind: EmbeddingProviderKind::Remote,
            remote_base_url: Some("https://api.example.com".to_string()),
            remote_model: Some("text-embedding-3-small".to_string()),
            ..Default::default()
        };
        let Err(err) = provider_for(&config, None) else {
            panic!("expected an error");
        };
        assert!(matches!(err, EmbeddingError::Message(_)));
    }
}
