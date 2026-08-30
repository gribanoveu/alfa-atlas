//! Product metrics: event shape, dimension-slot registry and the collector
//! configuration.
//!
//! The event contract (the `custom_dimension` iglu schema, the `Metric`
//! shape, the `'1'`-is-`organizationId` slot convention, the
//! `corp-<app>-ui` / `..._dev` appId convention) is taken from
//! `alfa-metrics-kit` in `corp-wlbuh-ausn-ndfl-ui`, itself a port of
//! `@alfa-bank/common-app-html`. The *transport* is deliberately not:
//! that kit works by loading a remote `sp.js` into the page, which does
//! not survive a Tauri webview (see `METRICS.md`). Everything below the
//! event contract is ours — see `infra::snowplow_client`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::settings::SettingsError;

/// Corp custom-dimension schema — the envelope every event's dimension
/// slots travel in.
pub const CUSTOM_DIMENSION_SCHEMA: &str = "iglu:com.alfabank/custom_dimension/jsonschema/1-0-0";

/// Snowplow tracker-protocol envelopes.
pub const PAYLOAD_DATA_SCHEMA: &str =
    "iglu:com.snowplowanalytics.snowplow/payload_data/jsonschema/1-0-4";
pub const CONTEXTS_SCHEMA: &str = "iglu:com.snowplowanalytics.snowplow/contexts/jsonschema/1-0-1";

/// Baked-in metrics configuration, from the `metrics` section of
/// `assets/llm/system_providers.yaml`. Same mechanism as the LLM and
/// embedding presets: a fork rebrands (or disables) metrics by editing
/// that file, without touching any `.rs`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsPreset {
    /// Release builds report as `app_id`, debug builds as `dev_app_id`.
    /// Both go to the same `domain`: separating dev traffic is what the
    /// two ids are for, and a second endpoint would add nothing.
    pub app_id: String,
    pub dev_app_id: String,
    pub domain: String,
    #[serde(default = "default_metrics_type")]
    pub metrics_type: String,
    #[serde(default = "default_platform")]
    pub platform: String,
    /// Trust root for the collector, replacing the agent's default store
    /// when set. `None` falls back to the bundled WebPki roots — the same
    /// contract every LLM provider has.
    #[serde(default)]
    pub trusted_cert_pem: Option<String>,
}

fn default_metrics_type() -> String {
    "corp".to_string()
}

fn default_platform() -> String {
    "web".to_string()
}

/// In corp web apps slot `'1'` carries `organizationId`. A desktop app has
/// no organization context, so it is left empty here — and, more
/// importantly, never reused for something else, so the same slot keeps
/// the same meaning across every Alfa Metrics consumer.
pub const ORGANIZATION_DIMENSION_SLOT: &str = "1";

/// Occupied dimension slots. Keep this list and the table in `METRICS.md`
/// in step: a slot silently reused for a second meaning makes every
/// historical query over it wrong.
pub const SLOT_INSTALL_ID: &str = "2";
pub const SLOT_APP_VERSION: &str = "3";
/// Operating system the app runs on. Named `os`, not `platform`: the
/// tracker protocol's own `p` field already means "platform" and carries
/// `web` for every event.
pub const SLOT_OS: &str = "4";
/// One app launch. Random per run, never persisted — it says nothing new
/// about the user beyond the install id, but it lets events from one run
/// be stitched into a funnel (opened a project → asked the assistant →
/// committed). Without it every event is an unconnected point.
pub const SLOT_SESSION_ID: &str = "5";
/// Which LLM provider a turn ran against. Cross-cutting: the same value
/// slices assistant turns, tool runs, failures and setup events alike.
///
/// Only ever holds a **system** provider id from the bundled manifest.
/// A user-configured provider is reported as `custom` — its id is a name
/// the user typed and can carry an internal hostname or their own name.
/// See `services::metrics::sanitize_dimensions`.
pub const SLOT_PROVIDER: &str = "6";

/// Stand-in reported for any provider that is not in the bundled manifest.
pub const PROVIDER_CUSTOM: &str = "custom";

/// A single struct event. Mirrors `Metric` + resolved `dimensionsMapping`
/// from the kit: the frontend does the `{reportId: 'abc'} + {reportId:
/// '2'}` → `{'2': 'abc'}` mapping (`src/lib/metrics/mapDimensions.ts`) and
/// sends the resolved slots, so this side never has to know event-specific
/// key names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricEvent {
    pub category: String,
    pub action: String,
    pub label: String,
    #[serde(default)]
    pub property: Option<String>,
    #[serde(default)]
    pub value: Option<f64>,
    /// Slots `'2'`–`'20'`. `BTreeMap` rather than `HashMap` so a built
    /// payload is byte-stable and therefore assertable in tests.
    #[serde(default)]
    pub dimensions: BTreeMap<String, String>,
}

/// Which collector this build talks to, and as whom.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricsConfig {
    pub app_id: String,
    /// `https://<domain>/metrica/corp` — the tracker-protocol path is
    /// appended by `infra::snowplow_client`.
    pub collector_base: String,
    pub platform: String,
    /// Snowplow's `tv` field. Ours is not a `js-*` tracker, and saying so
    /// honestly keeps desktop traffic separable from web traffic in the
    /// warehouse.
    pub tracker_version: String,
    /// See `MetricsPreset::trusted_cert_pem`.
    pub trusted_cert_pem: Option<String>,
}

impl MetricsConfig {
    /// Debug builds report under the `_dev` appId, so a developer running
    /// `tauri dev` never shows up as real usage. Both builds post to the
    /// same collector — the appId is the whole separation, and dashboards
    /// filter on it.
    pub fn from_preset(preset: &MetricsPreset, is_production: bool) -> Self {
        let app_id = if is_production {
            &preset.app_id
        } else {
            &preset.dev_app_id
        };
        Self {
            app_id: app_id.clone(),
            collector_base: format!(
                "https://{}/metrica/{}",
                preset.domain, preset.metrics_type
            ),
            platform: preset.platform.clone(),
            tracker_version: format!("atlas-{}", env!("CARGO_PKG_VERSION")),
            trusted_cert_pem: preset.trusted_cert_pem.clone(),
        }
    }

    /// `None` when this build ships no `metrics` manifest section, i.e.
    /// telemetry is compiled out entirely.
    pub fn resolve() -> Option<Self> {
        crate::infra::llm_provider_manifest::metrics_preset()
            .map(|preset| Self::from_preset(preset, !cfg!(debug_assertions)))
    }
}

/// Persisted in `~/.atlas/metrics.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsState {
    /// Anonymous UUID v4, generated on first read. Not derived from the
    /// user's name, email, hostname or any repository — see `METRICS.md`.
    #[serde(default)]
    pub install_id: Option<String>,
    /// Unix ms of the *confirmed* install-event delivery. `Some` is the
    /// only thing that stops the event being retried, so it must never be
    /// written on an unconfirmed send.
    #[serde(default)]
    pub install_reported_at: Option<i64>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// The run currently in progress, checkpointed to disk so a session
    /// that never exits cleanly is still reported. `None` between runs.
    #[serde(default)]
    pub open_session: Option<OpenSession>,
}

/// A run in progress. Written locally roughly once a minute while the app
/// is actually being used — **not** sent anywhere on that cadence. The
/// event still goes out once, at the end; this only makes sure "the end"
/// survives the app being killed rather than closed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSession {
    pub session_id: String,
    pub started_ms: i64,
    /// Seconds the window had focus as of the last checkpoint.
    pub active_secs: f64,
    /// When that checkpoint was taken — stands in for the exit time when
    /// the app never got to record one.
    pub last_seen_ms: i64,
}

fn default_enabled() -> bool {
    true
}

impl Default for MetricsState {
    fn default() -> Self {
        Self {
            install_id: None,
            install_reported_at: None,
            enabled: default_enabled(),
            open_session: None,
        }
    }
}

/// What the frontend needs to render the settings toggle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsStatus {
    pub enabled: bool,
    pub install_id: Option<String>,
    pub install_reported_at: Option<i64>,
}

impl From<MetricsState> for MetricsStatus {
    fn from(state: MetricsState) -> Self {
        Self {
            enabled: state.enabled,
            install_id: state.install_id,
            install_reported_at: state.install_reported_at,
        }
    }
}

#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("metrics storage error: {0}")]
    Storage(#[from] SettingsError),
    #[error("metrics transport error: {0}")]
    Transport(String),
    #[error("metrics collector returned http status {status}: {body}")]
    Status { status: u16, body: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset() -> MetricsPreset {
        MetricsPreset {
            app_id: "app".to_string(),
            dev_app_id: "app_dev".to_string(),
            domain: "collector.example".to_string(),
            metrics_type: "corp".to_string(),
            platform: "web".to_string(),
            trusted_cert_pem: Some("pem".to_string()),
        }
    }

    #[test]
    fn a_release_build_reports_under_the_production_appid() {
        let config = MetricsConfig::from_preset(&preset(), true);
        assert_eq!(config.app_id, "app");
        assert_eq!(config.trusted_cert_pem.as_deref(), Some("pem"));
        assert!(config.tracker_version.starts_with("atlas-"));
    }

    #[test]
    fn a_debug_build_reports_under_the_dev_appid() {
        let config = MetricsConfig::from_preset(&preset(), false);
        assert_eq!(config.app_id, "app_dev");
    }

    #[test]
    fn both_builds_use_the_same_collector() {
        let release = MetricsConfig::from_preset(&preset(), true);
        let debug = MetricsConfig::from_preset(&preset(), false);
        assert_eq!(release.collector_base, "https://collector.example/metrica/corp");
        assert_eq!(
            release.collector_base, debug.collector_base,
            "appId is the only thing that separates debug from release"
        );
    }

    /// Deliberately not an exact-value assertion on the appId — that is a
    /// config knob owned by `system_providers.yaml`, and pinning its
    /// literal here turns a legitimate rename into a broken build. What
    /// must hold is that a debug binary resolves to the *dev* id and never
    /// to the production collector.
    #[test]
    fn resolve_uses_the_bundled_preset_and_the_dev_appid_in_debug() {
        let preset =
            crate::infra::llm_provider_manifest::metrics_preset().expect("a metrics section");
        // The test binary is a debug build, so this must be the dev branch.
        let config = MetricsConfig::resolve().expect("this build ships a metrics section");
        assert_eq!(config.app_id, preset.dev_app_id);
        assert_ne!(config.app_id, preset.app_id);
        assert!(
            !config.collector_base.contains("metrics.alfabank.ru"),
            "the production collector is deliberately not used: {}",
            config.collector_base
        );
    }

    #[test]
    fn state_defaults_to_enabled_with_nothing_reported() {
        let state = MetricsState::default();
        assert!(state.enabled);
        assert!(state.install_id.is_none());
        assert!(state.install_reported_at.is_none());
    }

    #[test]
    fn a_legacy_state_file_without_enabled_defaults_to_enabled() {
        let state: MetricsState = serde_json::from_str(r#"{"installId":"abc"}"#).unwrap();
        assert!(state.enabled);
        assert_eq!(state.install_id.as_deref(), Some("abc"));
    }

    #[test]
    fn state_round_trips_through_camel_case_json() {
        let state = MetricsState {
            install_id: Some("id".to_string()),
            install_reported_at: Some(1_756_400_000_000),
            enabled: false,
            open_session: None,
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("installReportedAt"), "unexpected json: {json}");
        assert_eq!(serde_json::from_str::<MetricsState>(&json).unwrap(), state);
    }
}
