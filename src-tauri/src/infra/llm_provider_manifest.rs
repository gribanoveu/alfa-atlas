//! Compiled-in registry of "system" LLM providers, embedded at compile
//! time from `assets/llm/system_llm_providers.json`.
//!
//! Embedding (rather than shipping as a Tauri bundle resource) sidesteps
//! the resource-path differences between `cargo tauri dev` and a bundled
//! build — the data is simply part of the binary, in dev and in production
//! alike (see `common_spec_assets.rs`/`dictionary_assets.rs` for the same
//! rationale applied elsewhere in this codebase).
//!
//! This is the mechanism a downstream fork/rebrand uses to change or
//! remove the app's baked-in LLM provider(s) **without touching any `.rs`
//! file**: edit or empty out `system_llm_providers.json` and rebuild. An
//! empty JSON array (`[]`) is a fully valid manifest — it just means no
//! system providers ship at all, and every provider a user sees is one
//! they configured themselves (see `domain::llm::LlmProviderConfig`).
//!
//! A system provider's entry here is only ever a *default* — a user can
//! override its `model`/`trusted_cert_pem` (and, technically, `base_url`)
//! per `services::llm_config::resolve_provider`'s merge, with the override
//! persisted in `AppSettings.llm`, not here. There is deliberately no
//! runtime "delete a system provider" — true removal is this file's job.

use std::sync::LazyLock;

use crate::domain::llm::LlmProviderPreset;

const MANIFEST_JSON: &str = include_str!("../../assets/llm/system_llm_providers.json");

static PARSED: LazyLock<Vec<LlmProviderPreset>> = LazyLock::new(|| {
    serde_json::from_str(MANIFEST_JSON)
        .expect("bundled system_llm_providers.json must be a valid JSON array of LlmProviderPreset")
});

/// Every system provider this build ships with — `&[]` if the manifest was
/// emptied out by a fork. Parsed once, for the process lifetime.
pub fn system_providers() -> &'static [LlmProviderPreset] {
    &PARSED
}

pub fn find_system_provider(id: &str) -> Option<&'static LlmProviderPreset> {
    PARSED.iter().find(|preset| preset.id == id)
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
        assert_eq!(alfagen.base_url, "https://alfagen.moscow.alfaintra.net/continue-dev");
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
            crate::infra::llm_providers::openai_compatible::build_agent(Some(pem)).is_ok(),
            "bundled alfagen trust cert must be valid, parseable PEM"
        );
    }

    #[test]
    fn find_system_provider_is_none_for_an_unknown_id() {
        assert!(find_system_provider("does-not-exist").is_none());
    }

    /// An empty array is a valid manifest shape on its own — exercised
    /// directly (not via `system_providers()`, since the real bundled file
    /// isn't empty) so a fork that ships `[]` can trust this path works.
    #[test]
    fn empty_manifest_array_is_valid() {
        let presets: Vec<LlmProviderPreset> = serde_json::from_str("[]").unwrap();
        assert!(presets.is_empty());
    }
}
