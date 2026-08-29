//! Compiled-in embedding provider preset, embedded at compile time from
//! the top-level `embedding` section of `assets/llm/system_providers.yaml`
//! (the same file that holds the LLM presets under `llm`).
//!
//! Embedding (rather than shipping as a Tauri bundle resource) sidesteps
//! the resource-path differences between `cargo tauri dev` and a bundled
//! build — see `llm_provider_manifest.rs` for the same rationale.
//!
//! This is a **global** default, independent of any LLM provider id. A
//! fork edits the `embedding` object (or empties its fields back to
//! `null`) and rebuilds; no `.rs` change needed. Explicit `null`s for
//! `baseUrl`/`model` mean "use the Local BGE-M3 provider" — see
//! `services::embedding_config::resolve_embedding_config`.

use std::sync::LazyLock;

use crate::domain::embeddings::EmbeddingPreset;
use crate::domain::llm::LlmProviderPreset;
use serde::Deserialize;

const MANIFEST_YAML: &str = include_str!("../../assets/llm/system_providers.yaml");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SystemProvidersManifest {
    #[serde(default)]
    #[allow(dead_code)]
    llm: Vec<LlmProviderPreset>,
    #[serde(default)]
    embedding: EmbeddingPreset,
}

static PARSED: LazyLock<EmbeddingPreset> = LazyLock::new(|| {
    let manifest: SystemProvidersManifest = serde_yaml::from_str(MANIFEST_YAML)
        .expect("bundled system_providers.yaml must be a valid SystemProvidersManifest");
    manifest.embedding
});

/// The bundled embedding preset for this build. Fields are typically all
/// `None` until a fork fills them in — resolve treats that as Local.
pub fn system_embedding_preset() -> &'static EmbeddingPreset {
    &PARSED
}

/// Whether the preset alone implies a Remote provider: both `base_url` and
/// `model` must be non-empty. Empty/`None` either way means Local.
pub fn preset_implies_remote(preset: &EmbeddingPreset) -> bool {
    preset
        .base_url
        .as_ref()
        .is_some_and(|s| !s.trim().is_empty())
        && preset.model.as_ref().is_some_and(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_manifest_parses_without_panicking() {
        let _ = system_embedding_preset();
    }

    #[test]
    fn bundled_embedding_trust_cert_is_present_and_parses() {
        let preset = system_embedding_preset();
        let pem = preset
            .trusted_cert_pem
            .as_deref()
            .expect("embedding preset ships with a trusted cert");
        assert!(pem.contains("BEGIN CERTIFICATE"));
        assert!(
            crate::infra::http_agent::build_agent(Some(pem)).is_ok(),
            "bundled embedding trust cert must be valid, parseable PEM"
        );
    }

    #[test]
    fn preset_implies_remote_requires_both_base_url_and_model() {
        assert!(!preset_implies_remote(&EmbeddingPreset {
            base_url: Some("https://example.com/v1".into()),
            model: None,
            ..Default::default()
        }));
        assert!(!preset_implies_remote(&EmbeddingPreset {
            base_url: None,
            model: Some("emb".into()),
            ..Default::default()
        }));
        assert!(!preset_implies_remote(&EmbeddingPreset {
            base_url: Some("  ".into()),
            model: Some("emb".into()),
            ..Default::default()
        }));
        assert!(preset_implies_remote(&EmbeddingPreset {
            base_url: Some("https://example.com/v1".into()),
            model: Some("emb".into()),
            ..Default::default()
        }));
    }

    #[test]
    fn manifest_rejects_api_key_field_in_embedding_preset() {
        let err = serde_yaml::from_str::<SystemProvidersManifest>(
            r#"
llm: []
embedding:
  baseUrl: https://example.com/v1
  model: m
  apiKey: secret
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }
}
