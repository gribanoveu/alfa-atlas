//! Compiled-in registry of "system" LLM providers, embedded at compile
//! time from the top-level `llm` section of
//! `assets/llm/system_providers.yaml` (the same file that holds the
//! global embedding preset under `embedding` and baked-in API rate-limit
//! rules under `rateLimits`).
//!
//! Embedding (rather than shipping as a Tauri bundle resource) sidesteps
//! the resource-path differences between `cargo tauri dev` and a bundled
//! build — the data is simply part of the binary, in dev and in production
//! alike (see `common_spec_assets.rs`/`dictionary_assets.rs` for the same
//! rationale applied elsewhere in this codebase).
//!
//! This is the mechanism a downstream fork/rebrand uses to change or
//! remove the app's baked-in LLM provider(s) **without touching any `.rs`
//! file**: edit or empty out the `llm` array and rebuild. An empty JSON
//! array (`[]`) is a fully valid `llm` section — it just means no system
//! providers ship at all, and every provider a user sees is one they
//! configured themselves (see `domain::llm::LlmProviderConfig`).
//!
//! A system provider's entry here is only ever a *default* — a user can
//! override its `model`/`trusted_cert_pem` (and, technically, `base_url`)
//! per `services::llm_config::resolve_provider`'s merge, with the override
//! persisted in `AppSettings.llm`, not here. There is deliberately no
//! runtime "delete a system provider" — true removal is this file's job.

use std::sync::LazyLock;

use crate::domain::embeddings::EmbeddingPreset;
use crate::domain::llm::LlmProviderPreset;
use crate::domain::llm_rate_limit::RateLimitPreset;
use serde::Deserialize;

const MANIFEST_YAML: &str = include_str!("../../assets/llm/system_providers.yaml");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SystemProvidersManifest {
    #[serde(default)]
    llm: Vec<LlmProviderPreset>,
    #[serde(default)]
    #[allow(dead_code)]
    embedding: EmbeddingPreset,
    /// Baked-in API rate-limit rules, keyed by provider id. Empty means
    /// no status-bar chip for anyone — a fork clears this the same way it
    /// clears `llm`.
    #[serde(default)]
    rate_limits: Vec<RateLimitPreset>,
}

static PARSED: LazyLock<SystemProvidersManifest> = LazyLock::new(|| {
    serde_yaml::from_str(MANIFEST_YAML)
        .expect("bundled system_providers.yaml must be a valid SystemProvidersManifest")
});

/// Every system provider this build ships with — `&[]` if the `llm`
/// section was emptied out by a fork. Parsed once, for the process lifetime.
pub fn system_providers() -> &'static [LlmProviderPreset] {
    &PARSED.llm
}

pub fn find_system_provider(id: &str) -> Option<&'static LlmProviderPreset> {
    PARSED.llm.iter().find(|preset| preset.id == id)
}

pub fn rate_limit_presets() -> &'static [RateLimitPreset] {
    &PARSED.rate_limits
}

pub fn find_rate_limit(provider_id: &str) -> Option<&'static RateLimitPreset> {
    rate_limit_presets().iter().find(|preset| preset.provider_id == provider_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_manifest_parses_without_panicking() {
        // The assertion is that `system_providers()` doesn't panic — it
        // forces `PARSED`'s `LazyLock` init, which `.expect()`s on
        // malformed JSON.
        let _ = system_providers();
    }

    #[test]
    fn bundled_manifest_contains_alfagen() {
        let alfagen = find_system_provider("alfagen").expect("alfagen preset present");
        assert_eq!(alfagen.label, "AlfaGen");
        assert_eq!(alfagen.base_url, "https://alfagen.moscow.alfaintra.net/continue-dev/v1");
        assert_eq!(alfagen.request_headers, None);

        // Deliberately not an exact-value assertion. The token limits are a
        // *tuning* knob — they track what the endpoint currently serves and
        // are expected to be edited (this test was left asserting 200k
        // after the manifest moved to 260k, which is a broken build over a
        // legitimate config change, not a caught bug). What must hold is
        // that the numbers stay coherent: a mistyped `26000`/`2600000` or a
        // context smaller than the output reservation would break real
        // requests, and that is what's checked here.
        let limit = alfagen.limit.expect("alfagen ships with a token limit");
        assert!(limit.output > 0, "output reservation must be positive");
        assert!(
            limit.context > limit.output,
            "context window ({}) must exceed the output reservation ({})",
            limit.context,
            limit.output
        );
        assert!(
            (32_000..=2_000_000).contains(&limit.context),
            "context window {} is outside any plausible range — likely a typo",
            limit.context
        );
    }

    /// AlfaGen sits behind an internal corporate CA (`Alfa-Bank ST CA
    /// Root`), not the public web trust store — the bundled
    /// `trusted_cert_pem` is that root, captured directly from what the
    /// endpoint's own TLS handshake presents
    /// (`openssl s_client -connect alfagen.moscow.alfaintra.net:443
    /// -showcerts`, certificate index 2 — the self-signed root the server
    /// happens to also send). Round-tripped through `build_agent` here (not
    /// just asserted non-empty) so a future accidental edit that corrupts
    /// the PEM — a bad line ending, a truncated copy-paste — fails a test
    /// instead of silently shipping a cert that can't be parsed.
    #[test]
    fn bundled_manifest_alfagen_trust_cert_is_present_and_parses() {
        let alfagen = find_system_provider("alfagen").expect("alfagen preset present");
        let pem = alfagen.trusted_cert_pem.as_deref().expect("alfagen ships with a trusted cert");
        assert!(pem.contains("BEGIN CERTIFICATE"));
        assert!(
            crate::infra::http_agent::build_agent(Some(pem)).is_ok(),
            "bundled alfagen trust cert must be valid, parseable PEM"
        );
    }

    #[test]
    fn find_system_provider_is_none_for_an_unknown_id() {
        assert!(find_system_provider("does-not-exist").is_none());
    }

    /// An empty `llm` array is a valid manifest shape on its own — exercised
    /// directly (not via `system_providers()`, since the real bundled file
    /// isn't empty) so a fork that ships `"llm": []` can trust this path works.
    #[test]
    fn empty_llm_array_is_valid() {
        let manifest: SystemProvidersManifest =
            serde_yaml::from_str("llm: []\nembedding: {}\n").unwrap();
        assert!(manifest.llm.is_empty());
        assert!(manifest.rate_limits.is_empty());
    }

    #[test]
    fn bundled_manifest_contains_evc_rate_limit_for_alfagen() {
        assert!(
            rate_limit_presets().iter().any(|p| p.provider_id == "alfagen"),
            "rate_limit_presets() must surface the same baked-in rules as find_rate_limit"
        );
        let preset = find_rate_limit("alfagen").expect("alfagen rate limit present");
        assert_eq!(preset.policy_id, "evc-sliding-window");
        assert_eq!(preset.label, "EVC");
        assert_eq!(preset.limit, 60_000);
        assert_eq!(preset.window_minutes, 30);
        assert_eq!(preset.work_from_hour, None);
        assert_eq!(preset.work_to_hour, None);
        assert_eq!(preset.timezone_offset_hours, 3);
    }

    #[test]
    fn manifest_rejects_api_key_field_in_llm_preset() {
        let err = serde_yaml::from_str::<SystemProvidersManifest>(
            r#"
llm:
  - id: x
    label: X
    baseUrl: https://example.com/v1
    apiKey: secret
embedding: {}
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }
}
