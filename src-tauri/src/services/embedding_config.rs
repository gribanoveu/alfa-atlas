//! Loads/saves `AppSettings.embedding` (override layer) and resolves it
//! against the bundled `embedding` preset into the merged
//! `ResolvedEmbeddingConfig` the rest of the app actually uses.

use crate::domain::embeddings::{
    EmbeddingPreset, EmbeddingProviderConfig, EmbeddingProviderKind, ResolvedEmbeddingConfig,
};
use crate::domain::settings::SettingsError;
use crate::infra::embedding_provider_manifest;
use crate::infra::settings_store;

pub fn load_embedding_settings() -> Result<EmbeddingProviderConfig, SettingsError> {
    Ok(settings_store::load()?.embedding)
}

pub fn save_embedding_settings(config: EmbeddingProviderConfig) -> Result<(), SettingsError> {
    let mut settings = settings_store::load().unwrap_or_default();
    settings.embedding = config;
    settings_store::save(&settings)
}

/// Merges the bundled embedding preset with the settings-layer override —
/// override field wins whenever `Some`, `None` means inherit from the
/// preset. When neither pins `kind`, a preset with both non-empty
/// `base_url` and `model` implies Remote; otherwise Local.
pub fn resolve_embedding_config() -> Result<ResolvedEmbeddingConfig, SettingsError> {
    let settings = load_embedding_settings().unwrap_or_default();
    Ok(resolve_with(embedding_provider_manifest::system_embedding_preset(), &settings))
}

pub fn resolve_with(
    preset: &EmbeddingPreset,
    settings: &EmbeddingProviderConfig,
) -> ResolvedEmbeddingConfig {
    let kind = settings.kind.unwrap_or_else(|| {
        if embedding_provider_manifest::preset_implies_remote(preset) {
            EmbeddingProviderKind::Remote
        } else {
            EmbeddingProviderKind::Local
        }
    });

    ResolvedEmbeddingConfig {
        kind,
        remote_base_url: settings
            .remote_base_url
            .clone()
            .or_else(|| preset.base_url.clone()),
        remote_model: settings
            .remote_model
            .clone()
            .or_else(|| preset.model.clone()),
        remote_dimensions: settings.remote_dimensions.or(preset.dimensions),
        remote_trusted_cert_pem: settings
            .remote_trusted_cert_pem
            .clone()
            .or_else(|| preset.trusted_cert_pem.clone()),
        remote_system_id: settings
            .remote_system_id
            .clone()
            .or_else(|| preset.system_id.clone()),
        remote_disable_tls_verification: settings
            .remote_disable_tls_verification
            .or(preset.disable_tls_verification)
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_preset() -> EmbeddingPreset {
        EmbeddingPreset::default()
    }

    fn remote_preset() -> EmbeddingPreset {
        EmbeddingPreset {
            base_url: Some("https://emb.example/v1".into()),
            model: Some("emb-model".into()),
            dimensions: Some(1024),
            trusted_cert_pem: Some("-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n".into()),
            ..Default::default()
        }
    }

    #[test]
    fn null_preset_resolves_to_local() {
        let resolved = resolve_with(&empty_preset(), &EmbeddingProviderConfig::default());
        assert_eq!(resolved.kind, EmbeddingProviderKind::Local);
        assert_eq!(resolved.remote_base_url, None);
        assert_eq!(resolved.remote_model, None);
    }

    #[test]
    fn filled_preset_resolves_to_remote() {
        let resolved = resolve_with(&remote_preset(), &EmbeddingProviderConfig::default());
        assert_eq!(resolved.kind, EmbeddingProviderKind::Remote);
        assert_eq!(resolved.remote_base_url.as_deref(), Some("https://emb.example/v1"));
        assert_eq!(resolved.remote_model.as_deref(), Some("emb-model"));
        assert_eq!(resolved.remote_dimensions, Some(1024));
        assert!(resolved.remote_trusted_cert_pem.is_some());
    }

    #[test]
    fn settings_kind_local_overrides_remote_preset() {
        let settings = EmbeddingProviderConfig {
            kind: Some(EmbeddingProviderKind::Local),
            ..Default::default()
        };
        let resolved = resolve_with(&remote_preset(), &settings);
        assert_eq!(resolved.kind, EmbeddingProviderKind::Local);
        // Remote fields are still inherited — useful if the user flips back
        // to Remote later — but kind pin wins.
        assert_eq!(resolved.remote_base_url.as_deref(), Some("https://emb.example/v1"));
    }

    #[test]
    fn settings_kind_remote_overrides_null_preset() {
        let settings = EmbeddingProviderConfig {
            kind: Some(EmbeddingProviderKind::Remote),
            remote_base_url: Some("https://custom/v1".into()),
            remote_model: Some("custom".into()),
            ..Default::default()
        };
        let resolved = resolve_with(&empty_preset(), &settings);
        assert_eq!(resolved.kind, EmbeddingProviderKind::Remote);
        assert_eq!(resolved.remote_base_url.as_deref(), Some("https://custom/v1"));
        assert_eq!(resolved.remote_model.as_deref(), Some("custom"));
    }

    #[test]
    fn partial_override_replaces_only_that_field() {
        let settings = EmbeddingProviderConfig {
            remote_model: Some("override-model".into()),
            ..Default::default()
        };
        let resolved = resolve_with(&remote_preset(), &settings);
        assert_eq!(resolved.kind, EmbeddingProviderKind::Remote);
        assert_eq!(resolved.remote_base_url.as_deref(), Some("https://emb.example/v1"));
        assert_eq!(resolved.remote_model.as_deref(), Some("override-model"));
        assert_eq!(resolved.remote_dimensions, Some(1024));
    }

    #[test]
    fn bundled_preset_merges_with_empty_override() {
        let preset = embedding_provider_manifest::system_embedding_preset();
        let resolved = resolve_with(preset, &EmbeddingProviderConfig::default());
        let expected_kind = if embedding_provider_manifest::preset_implies_remote(preset) {
            EmbeddingProviderKind::Remote
        } else {
            EmbeddingProviderKind::Local
        };
        assert_eq!(resolved.kind, expected_kind);
        assert_eq!(resolved.remote_base_url, preset.base_url.clone());
        assert_eq!(resolved.remote_model, preset.model.clone());
        assert_eq!(resolved.remote_dimensions, preset.dimensions);
        assert_eq!(resolved.remote_trusted_cert_pem, preset.trusted_cert_pem.clone());
        assert_eq!(resolved.remote_system_id, preset.system_id.clone());
        assert_eq!(
            resolved.remote_disable_tls_verification,
            preset.disable_tls_verification.unwrap_or(false)
        );
    }
}
