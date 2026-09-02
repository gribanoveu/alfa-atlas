//! Jira settings: the two-layer merge (build manifest under the user's own
//! settings) plus the one use case built on top of it — resolve settings +
//! stored token into a client and ask Jira who the token belongs to.
//!
//! Load/save mirror `services::spellcheck_prefs` (a section of
//! `~/.atlas/settings.json`); the merge mirrors
//! `services::llm_config::resolve_provider` (a manifest preset folded with
//! a settings-layer override). The token never passes through here as data,
//! only as something fetched from `infra::jira_credentials_store` at the
//! moment a request is made.

use crate::domain::jira::{JiraError, JiraPreset, JiraSettings, JiraSettingsView, JiraUser};
use crate::domain::settings::SettingsError;
use crate::infra::{
    jira_client, jira_credentials_store, llm_provider_manifest, settings_store,
};

/// The user layer alone — what the settings form edits.
pub fn load_jira_settings() -> Result<JiraSettings, SettingsError> {
    Ok(settings_store::load()?.jira)
}

/// The user layer plus what the build would fall back to, so the settings
/// tab can say "задаётся сборкой" instead of showing an empty field that
/// nonetheless works.
pub fn load_jira_settings_view() -> Result<JiraSettingsView, SettingsError> {
    let preset = llm_provider_manifest::jira_preset();
    Ok(JiraSettingsView {
        settings: load_jira_settings()?,
        bundled_base_url: preset.base_url.as_deref().and_then(non_empty),
        has_bundled_cert: preset
            .trusted_cert_pem
            .as_deref()
            .and_then(non_empty)
            .is_some(),
    })
}

/// Normalizes before writing so everything downstream can assume trimmed
/// values, and a field someone cleared to whitespace reads back as "no
/// override" (falling back to the build preset) rather than as an empty
/// string that would shadow it.
pub fn save_jira_settings(settings: JiraSettings) -> Result<(), SettingsError> {
    let mut all = settings_store::load().unwrap_or_default();
    all.jira = JiraSettings {
        base_url: settings.base_url.trim().trim_end_matches('/').to_string(),
        trusted_cert_pem: settings
            .trusted_cert_pem
            .as_deref()
            .and_then(non_empty),
    };
    settings_store::save(&all)
}

/// The effective connection: each user field, or the build's default when
/// that field is empty. Same precedence as `llm_config::resolve_provider` —
/// override wins, preset fills the gap.
pub fn resolve(settings: &JiraSettings, preset: &JiraPreset) -> JiraSettings {
    JiraSettings {
        base_url: non_empty(&settings.base_url)
            .or_else(|| preset.base_url.as_deref().and_then(non_empty))
            .unwrap_or_default(),
        trusted_cert_pem: settings
            .trusted_cert_pem
            .as_deref()
            .and_then(non_empty)
            .or_else(|| preset.trusted_cert_pem.as_deref().and_then(non_empty)),
    }
}

/// The account behind the stored token — both the right-dock panel's content
/// and its connection check, since there is nothing to show unless the round
/// trip succeeded. Blocking; callers run it on a blocking thread.
pub fn current_user() -> Result<JiraUser, JiraError> {
    let stored = load_jira_settings().map_err(|e| JiraError::Settings(e.to_string()))?;
    let settings = resolve(&stored, llm_provider_manifest::jira_preset());
    if !settings.is_addressable() {
        return Err(JiraError::NotConfigured);
    }
    let token = jira_credentials_store::get_token().ok_or(JiraError::MissingToken)?;

    let jira = jira_client::connect(&settings, token)?;
    jira_client::current_user(&jira)
}

/// Trimmed, or `None` when blank — the single rule for "this field carries
/// no value", applied to both layers.
fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;

    fn preset() -> JiraPreset {
        JiraPreset {
            base_url: Some("https://jira.build.example".to_string()),
            trusted_cert_pem: Some("BUILD PEM".to_string()),
        }
    }

    #[test]
    fn saving_trims_the_base_url_and_drops_its_trailing_slash() {
        with_temp_home(|| {
            save_jira_settings(JiraSettings {
                base_url: "  https://jira.example.com/  ".to_string(),
                trusted_cert_pem: Some("   ".to_string()),
            })
            .unwrap();

            let loaded = load_jira_settings().unwrap();
            assert_eq!(loaded.base_url, "https://jira.example.com");
            // Whitespace is not an override — the build preset stays in play.
            assert_eq!(loaded.trusted_cert_pem, None);
        });
    }

    #[test]
    fn the_build_preset_fills_fields_the_user_left_empty() {
        let resolved = resolve(&JiraSettings::default(), &preset());
        assert_eq!(resolved.base_url, "https://jira.build.example");
        assert_eq!(resolved.trusted_cert_pem.as_deref(), Some("BUILD PEM"));
    }

    #[test]
    fn user_values_win_over_the_build_preset() {
        let resolved = resolve(
            &JiraSettings {
                base_url: "https://jira.mine.example".to_string(),
                trusted_cert_pem: Some("MY PEM".to_string()),
            },
            &preset(),
        );
        assert_eq!(resolved.base_url, "https://jira.mine.example");
        assert_eq!(resolved.trusted_cert_pem.as_deref(), Some("MY PEM"));
    }

    #[test]
    fn an_empty_preset_leaves_an_unconfigured_instance_unconfigured() {
        let resolved = resolve(&JiraSettings::default(), &JiraPreset::default());
        assert!(!resolved.is_addressable());
        assert_eq!(resolved.trusted_cert_pem, None);
    }

    #[test]
    fn an_unconfigured_instance_never_reaches_the_network() {
        with_temp_home(|| {
            // Guards the manifest-shipped case too: if this build ever
            // starts shipping a `jira.baseUrl`, the assertion below moves to
            // `MissingToken`, which is still a pre-network refusal.
            let err = current_user().unwrap_err();
            assert!(
                matches!(err, JiraError::NotConfigured | JiraError::MissingToken),
                "unexpected error: {err}"
            );
        });
    }

    #[test]
    fn a_configured_instance_without_a_token_asks_for_one() {
        with_temp_home(|| {
            save_jira_settings(JiraSettings {
                base_url: "https://jira.example.com".to_string(),
                trusted_cert_pem: None,
            })
            .unwrap();

            let err = current_user().unwrap_err();
            assert!(matches!(err, JiraError::MissingToken));
        });
    }
}
