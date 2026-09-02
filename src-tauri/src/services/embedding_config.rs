//! Loads/saves `AppSettings.embedding` (override layer) and resolves it
//! against the bundled `embedding` preset into the merged
//! `ResolvedEmbeddingConfig` the rest of the app actually uses.

use std::collections::HashMap;

use crate::domain::embeddings::{
    EmbeddingError, EmbeddingPreset, EmbeddingProviderConfig, EmbeddingProviderKind,
    REQUEST_HEADER_VALUE_UUID, ResolvedEmbeddingConfig,
};
use crate::domain::settings::SettingsError;
use crate::infra::embedding_credentials_store;
use crate::infra::embedding_provider_manifest;
use crate::infra::embedding_providers;
use crate::infra::settings_store;

/// Text sent by `test_connection`. Deliberately trivial: the point is to
/// exercise the request path, and a remote endpoint bills by token.
const CONNECTION_PROBE_TEXT: &str = "test";

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
        // Unlike the other remote fields, dimensions is never settings-
        // overridable — like `local::DIMENSIONS`, it has to stay a fixed
        // constant of "which provider is running", not something a user
        // can retype independently. A usearch index is dimension-fixed, so
        // a dimension change orphans whatever's already embedded; letting
        // it come from mutable settings meant editing this field (or a
        // stale/blank value defaulting differently than intended) silently
        // wiped and re-embedded the whole index — via a paid API, on the
        // next app start — the moment the resolved value drifted from what
        // was persisted. Pinning it to the compiled-in preset removes that
        // failure mode entirely.
        remote_dimensions: preset.dimensions,
        remote_trusted_cert_pem: settings
            .remote_trusted_cert_pem
            .clone()
            .or_else(|| preset.trusted_cert_pem.clone()),
        remote_trusted_cert_override: cert_override(
            settings.remote_trusted_cert_pem.as_deref(),
            preset.trusted_cert_pem.as_deref(),
        ),
        has_bundled_cert: preset.trusted_cert_pem.is_some(),
        remote_request_headers: resolve_request_headers(preset, settings),
        remote_disable_tls_verification: settings
            .remote_disable_tls_verification
            .or(preset.disable_tls_verification)
            .unwrap_or(false),
        api_key_bundled: embedding_credentials_store::has_bundled_api_key(),
        api_key_user_set: embedding_credentials_store::has_user_key(),
    }
}

/// End-to-end check of the remote embedding setup: config → key → TLS →
/// HTTP → response parsing → vector dimension. Embeds one short probe
/// string, because `/embeddings` is the only endpoint the provider trait
/// exposes — there is no `/models` to poke at like the LLM providers have.
///
/// The dimension check is the part worth having. A `usearch` index is
/// dimension-fixed, so a model whose output width differs from the build's
/// `EmbeddingPreset::dimensions` cannot be used at all — and without this
/// the mismatch would only surface much later, mid-sync, after the index
/// had already been wiped.
///
/// Builds its own provider rather than going through
/// `services::embedding_state::ensure_provider`: the point is to test the
/// settings as they stand right now, not whatever a cached instance was
/// built from. Blocking; the caller runs it on a blocking thread.
pub fn test_connection() -> Result<String, EmbeddingError> {
    let config = resolve_embedding_config().map_err(|e| EmbeddingError::Message(e.to_string()))?;
    if config.kind != EmbeddingProviderKind::Remote {
        return Err(EmbeddingError::Message(
            "Локальная модель работает без сети — проверять нечего.".to_string(),
        ));
    }

    let provider = embedding_providers::provider_for(&config, embedding_credentials_store::get_api_key())?;
    let expected = provider.dimensions();
    let vectors = provider.embed(&[CONNECTION_PROBE_TEXT])?;

    let actual = vectors
        .first()
        .ok_or_else(|| {
            EmbeddingError::Message("Эндпоинт ответил без единого вектора.".to_string())
        })?
        .0
        .len();
    if actual != expected {
        return Err(EmbeddingError::Message(format!(
            "Модель вернула вектор размерности {actual}, а сборка рассчитана на {expected}. \
             Индекс с такой моделью работать не будет — нужна другая модель."
        )));
    }

    Ok(format!("Соединение установлено. Размерность вектора: {expected}."))
}

/// The settings-layer certificate, as the Settings form should see it —
/// `None` when there is nothing of the user's own.
///
/// A stored value byte-identical to the build's certificate is treated as
/// *no* override: it carries no information, and older builds wrote exactly
/// that. `useEmbeddingSetup.updateConfig` used to persist the **resolved**
/// certificate, so editing any unrelated field (model, headers) silently
/// pinned the bundled PEM as the user's own — and once pinned, a manifest
/// update would never reach that user again. Collapsing the two here heals
/// those settings on read, and the next save writes the cleaned value back.
fn cert_override(stored: Option<&str>, preset: Option<&str>) -> Option<String> {
    let stored = stored?;
    if Some(stored) == preset {
        return None;
    }
    Some(stored.to_string())
}

/// HTTP headers for remote `/embeddings`. Settings override replaces the
/// preset map entirely when set; otherwise the bundled preset applies,
/// including legacy `systemId` → `systemId` + `messageId: $uuid`.
pub fn resolve_request_headers(
    preset: &EmbeddingPreset,
    settings: &EmbeddingProviderConfig,
) -> HashMap<String, String> {
    if let Some(headers) = &settings.remote_request_headers {
        return headers.clone();
    }

    let mut headers = preset.request_headers.clone().unwrap_or_default();
    apply_legacy_system_id_from_preset(&mut headers, preset.system_id.as_deref());
    apply_legacy_system_id_from_settings(&mut headers, settings.remote_system_id.as_deref());
    headers
}

fn apply_legacy_system_id_from_preset(headers: &mut HashMap<String, String>, system_id: Option<&str>) {
    let Some(sid) = system_id.filter(|s| !s.trim().is_empty()) else {
        return;
    };
    headers
        .entry("systemId".to_string())
        .or_insert_with(|| sid.to_string());
    headers
        .entry("messageId".to_string())
        .or_insert_with(|| REQUEST_HEADER_VALUE_UUID.to_string());
}

/// Settings-layer legacy `remoteSystemId` wins over preset headers — same
/// per-field override semantics as the other remote settings fields.
fn apply_legacy_system_id_from_settings(headers: &mut HashMap<String, String>, system_id: Option<&str>) {
    let Some(sid) = system_id.filter(|s| !s.trim().is_empty()) else {
        return;
    };
    headers.insert("systemId".to_string(), sid.to_string());
    headers
        .entry("messageId".to_string())
        .or_insert_with(|| REQUEST_HEADER_VALUE_UUID.to_string());
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
        // …but the *override* stays empty, which is what the Settings form
        // binds to. Leaking the preset PEM into that field would make it
        // indistinguishable from the user's own, and saving the form would
        // pin the build's default so later manifest changes stop arriving.
        assert_eq!(resolved.remote_trusted_cert_override, None);
        assert!(resolved.has_bundled_cert);
    }

    /// Heals settings written by the older `updateConfig`, which persisted
    /// the *resolved* certificate — so editing any unrelated field pinned
    /// the bundled PEM as the user's own.
    #[test]
    fn a_stored_copy_of_the_build_certificate_is_not_an_override() {
        let preset = remote_preset();
        let settings = EmbeddingProviderConfig {
            remote_trusted_cert_pem: preset.trusted_cert_pem.clone(),
            ..Default::default()
        };
        let resolved = resolve_with(&preset, &settings);
        assert_eq!(resolved.remote_trusted_cert_override, None);
        // The client still gets a certificate — only the form sees nothing.
        assert_eq!(resolved.remote_trusted_cert_pem, preset.trusted_cert_pem);
    }

    #[test]
    fn a_user_certificate_shows_up_as_both_merged_and_override() {
        let settings = EmbeddingProviderConfig {
            remote_trusted_cert_pem: Some("MY PEM".into()),
            ..Default::default()
        };
        let resolved = resolve_with(&remote_preset(), &settings);
        assert_eq!(resolved.remote_trusted_cert_pem.as_deref(), Some("MY PEM"));
        assert_eq!(resolved.remote_trusted_cert_override.as_deref(), Some("MY PEM"));
        assert!(resolved.has_bundled_cert);
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
        assert_eq!(
            resolved.remote_request_headers,
            resolve_request_headers(preset, &EmbeddingProviderConfig::default())
        );
        assert_eq!(
            resolved.remote_disable_tls_verification,
            preset.disable_tls_verification.unwrap_or(false)
        );
    }

    #[test]
    fn settings_request_headers_override_replaces_preset() {
        let preset = EmbeddingPreset {
            request_headers: Some(HashMap::from([
                ("systemId".into(), "preset".into()),
                ("messageId".into(), REQUEST_HEADER_VALUE_UUID.into()),
            ])),
            ..remote_preset()
        };
        let settings = EmbeddingProviderConfig {
            remote_request_headers: Some(HashMap::from([("X-Custom".into(), "1".into())])),
            ..Default::default()
        };
        let resolved = resolve_with(&preset, &settings);
        assert_eq!(
            resolved.remote_request_headers,
            HashMap::from([("X-Custom".into(), "1".into())])
        );
    }

    #[test]
    fn legacy_system_id_expands_to_system_and_message_headers() {
        let preset = EmbeddingPreset {
            system_id: Some("sanduser".into()),
            ..remote_preset()
        };
        let resolved = resolve_with(&preset, &EmbeddingProviderConfig::default());
        assert_eq!(resolved.remote_request_headers.get("systemId").map(String::as_str), Some("sanduser"));
        assert_eq!(
            resolved.remote_request_headers.get("messageId").map(String::as_str),
            Some(REQUEST_HEADER_VALUE_UUID)
        );
    }

    #[test]
    fn legacy_settings_system_id_overrides_preset_request_headers() {
        let preset = EmbeddingPreset {
            request_headers: Some(HashMap::from([
                ("systemId".into(), "sanduser".into()),
                ("messageId".into(), REQUEST_HEADER_VALUE_UUID.into()),
            ])),
            ..remote_preset()
        };
        let settings = EmbeddingProviderConfig {
            remote_system_id: Some("custom-user".into()),
            ..Default::default()
        };
        let resolved = resolve_with(&preset, &settings);
        assert_eq!(
            resolved.remote_request_headers.get("systemId").map(String::as_str),
            Some("custom-user")
        );
        assert_eq!(
            resolved.remote_request_headers.get("messageId").map(String::as_str),
            Some(REQUEST_HEADER_VALUE_UUID)
        );
    }
}
