//! Calendar settings: the two-layer merge (build manifest under the user's
//! own settings) and building a connected `OwaSession` from settings + a
//! password. Mirrors `services::jira_config`. The password passes through
//! only at the moment a session is built — from the store when remembered,
//! or supplied by the user for this run only.

use crate::domain::calendar::{CalendarPreset, CalendarSettings, CalendarSettingsView};
use crate::domain::settings::SettingsError;
use crate::infra::{llm_provider_manifest, owa_client::OwaSession, settings_store};

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

pub fn load_settings() -> Result<CalendarSettings, SettingsError> {
    Ok(settings_store::load()?.calendar)
}

pub fn load_settings_view(has_password: bool) -> Result<CalendarSettingsView, SettingsError> {
    let preset = llm_provider_manifest::calendar_preset();
    Ok(CalendarSettingsView {
        settings: load_settings()?,
        bundled_base_url: preset.base_url.as_deref().and_then(non_empty),
        has_bundled_cert: preset.trusted_cert_pem.as_deref().and_then(non_empty).is_some(),
        has_password,
    })
}

/// Normalizes before writing so downstream can assume trimmed values and a
/// field cleared to whitespace reads back as "no override".
pub fn save_settings(settings: CalendarSettings) -> Result<(), String> {
    // A domain password must only ever go over TLS to a named host the admin
    // set — reject http/IP before it is stored (redirect-leak guard).
    crate::domain::calendar::validate_base_url(&settings.base_url)?;
    let mut all = settings_store::load().unwrap_or_default();
    all.calendar = CalendarSettings {
        base_url: settings.base_url.trim().trim_end_matches('/').to_string(),
        username: settings.username.trim().to_string(),
        display_time_zone: settings.display_time_zone.trim().to_string(),
        remember_password: settings.remember_password,
        trusted_cert_pem: settings.trusted_cert_pem.as_deref().and_then(non_empty),
    };
    settings_store::save(&all).map_err(|e| e.to_string())
}

/// Settings folded with the build preset: user field wins, preset fills gaps.
pub fn resolve(settings: &CalendarSettings, preset: &CalendarPreset) -> CalendarSettings {
    CalendarSettings {
        base_url: non_empty(&settings.base_url)
            .or_else(|| preset.base_url.as_deref().and_then(non_empty))
            .unwrap_or_default(),
        username: settings.username.clone(),
        display_time_zone: settings.display_time_zone.clone(),
        remember_password: settings.remember_password,
        trusted_cert_pem: settings
            .trusted_cert_pem
            .as_deref()
            .and_then(non_empty)
            .or_else(|| preset.trusted_cert_pem.as_deref().and_then(non_empty)),
    }
}

/// Effective settings (post-resolve). `None` if nothing is addressable yet.
pub fn effective() -> Option<CalendarSettings> {
    let stored = load_settings().ok()?;
    let settings = resolve(&stored, llm_provider_manifest::calendar_preset());
    settings.is_addressable().then_some(settings)
}

/// Builds a session from effective settings + the given password. `None` when
/// the calendar is not addressable or has no username yet.
pub fn build_session(password: &str) -> Option<Result<OwaSession, crate::domain::calendar::CalendarError>> {
    let s = effective()?;
    if s.username.trim().is_empty() {
        return None;
    }
    Some(OwaSession::new(&s.base_url, &s.username, password, s.trusted_cert_pem.as_deref()))
}
