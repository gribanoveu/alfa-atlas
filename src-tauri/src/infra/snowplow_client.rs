//! The only place that knows the Snowplow tracker protocol.
//!
//! `alfa-metrics-kit` gets this for free by loading a remote `sp.js`, which
//! is not an option in a Tauri webview (`METRICS.md` explains why), so the
//! payload is built and posted here instead. The upside is that a send
//! either confirms or fails — the kit's `track.ts` silently drops events
//! whenever the remote tracker is absent, which for a desktop app that is
//! regularly off the corporate network would be most of them.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use uuid::Uuid;

use crate::domain::metrics::{
    MetricEvent, MetricsConfig, MetricsError, CONTEXTS_SCHEMA, CUSTOM_DIMENSION_SCHEMA,
    ORGANIZATION_DIMENSION_SLOT, PAYLOAD_DATA_SCHEMA,
};
use crate::infra::http_agent::build_agent;

/// Enough of a collector error to diagnose it, not enough to dump an HTML
/// error page into the log — same reasoning as
/// `llm_providers::openai_compatible::ERROR_BODY_MAX_CHARS`.
const ERROR_BODY_MAX_CHARS: usize = 500;

pub fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Builds the tracker-protocol body for one struct event.
///
/// Every value is a string: the protocol is form-encoded in its GET form,
/// and the POST form keeps the same string-typed shape, so a numeric
/// `se_va` sent as a JSON number is rejected downstream.
///
/// Split out from `send` so the payload can be asserted in tests without
/// touching the network.
/// Wraps already-built event field maps in the tracker-protocol envelope.
/// Takes a slice because the envelope is an array: a queue flush posts
/// everything pending in one request rather than one request per event.
pub fn wrap_payload(events: &[Value]) -> Value {
    json!({
        "schema": PAYLOAD_DATA_SCHEMA,
        "data": events,
    })
}

/// One struct event as the flat, string-typed field map the protocol
/// wants. Separate from `wrap_payload` so a queued event can be built now
/// and sent later, alongside others.
pub fn build_event_fields(
    config: &MetricsConfig,
    event: &MetricEvent,
    install_id: &str,
    event_id: &str,
    device_timestamp_ms: i64,
) -> Value {
    // Slot '1' means `organizationId` everywhere else in Alfa Metrics.
    // A desktop app has no organization, so putting anything there would
    // corrupt a dimension other consumers rely on.
    debug_assert!(
        !event.dimensions.contains_key(ORGANIZATION_DIMENSION_SLOT),
        "dimension slot '1' is reserved for organizationId"
    );

    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    fields.insert("e".to_string(), "se".to_string());
    fields.insert("se_ca".to_string(), event.category.clone());
    fields.insert("se_ac".to_string(), event.action.clone());
    fields.insert("se_la".to_string(), event.label.clone());
    if let Some(property) = &event.property {
        fields.insert("se_pr".to_string(), property.clone());
    }
    if let Some(value) = event.value {
        fields.insert("se_va".to_string(), value.to_string());
    }
    fields.insert("aid".to_string(), config.app_id.clone());
    fields.insert("p".to_string(), config.platform.clone());
    fields.insert("tv".to_string(), config.tracker_version.clone());
    fields.insert("eid".to_string(), event_id.to_string());
    fields.insert("dtm".to_string(), device_timestamp_ms.to_string());
    // `duid` (domain user id) rather than `uid` (business user id): the
    // install id identifies a device profile, not a person, and putting it
    // in `uid` would claim an identity the app does not have.
    fields.insert("duid".to_string(), install_id.to_string());

    if !event.dimensions.is_empty() {
        let contexts = json!({
            "schema": CONTEXTS_SCHEMA,
            "data": [{
                "schema": CUSTOM_DIMENSION_SCHEMA,
                "data": event.dimensions,
            }],
        });
        fields.insert("co".to_string(), contexts.to_string());
    }

    json!(fields)
}

/// Convenience for a single event — the install report's path.
pub fn build_payload(
    config: &MetricsConfig,
    event: &MetricEvent,
    install_id: &str,
    event_id: &str,
    device_timestamp_ms: i64,
) -> Value {
    let fields = build_event_fields(config, event, install_id, event_id, device_timestamp_ms);
    wrap_payload(&[fields])
}

/// POSTs one event and blocks until the collector answers. Blocking on
/// purpose: `ureq` is a blocking client and `tokio` is built here without
/// the `net` feature, so every caller wraps this in `spawn_blocking` — the
/// same pattern the LLM and embedding providers use.
///
/// Only a 2xx counts as delivered. A DNS failure off the VPN, a proxy
/// interception and a collector 500 all land in `Err`, which is what keeps
/// `services::metrics::report_install_once` from marking an undelivered
/// event as reported.
pub fn send(
    config: &MetricsConfig,
    event: &MetricEvent,
    install_id: &str,
) -> Result<(), MetricsError> {
    let payload = build_payload(
        config,
        event,
        install_id,
        &Uuid::new_v4().to_string(),
        unix_millis(),
    );
    post_payload(config, &payload)
}

/// POSTs an already-built envelope. Same blocking contract as `send`.
pub fn post_payload(config: &MetricsConfig, payload: &Value) -> Result<(), MetricsError> {
    let url = format!("{}/com.snowplowanalytics.snowplow/tp2", config.collector_base);

    // The collector's chain roots in a CA that is absent from the
    // bundled WebPki store, so the trust root travels with the manifest
    // (`system_providers.yaml` → `metrics.trustedCertPem`) exactly the way
    // an LLM provider's does. Without it the handshake fails with
    // `UnknownIssuer`.
    let agent = build_agent(config.trusted_cert_pem.as_deref())
        .map_err(|e| MetricsError::Transport(e.to_string()))?;
    let mut response = agent
        .post(&url)
        .header("Content-Type", "application/json; charset=UTF-8")
        .send_json(payload)
        .map_err(|e| MetricsError::Transport(e.to_string()))?;

    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    // `build_agent` disables ureq's status-to-error conversion, so the
    // body is still readable here and can go into the message.
    let body = response
        .body_mut()
        .read_to_string()
        .unwrap_or_else(|e| format!("<failed to read error response body: {e}>"));
    let truncated = if body.chars().count() > ERROR_BODY_MAX_CHARS {
        let head: String = body.chars().take(ERROR_BODY_MAX_CHARS).collect();
        format!("{head}… (truncated)")
    } else {
        body
    };
    Err(MetricsError::Status {
        status: status.as_u16(),
        body: truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::metrics::{SLOT_APP_VERSION, SLOT_INSTALL_ID, SLOT_OS};

    fn config() -> MetricsConfig {
        MetricsConfig {
            app_id: "alfa-atlas-ui_dev".to_string(),
            collector_base: "https://testjmb.alfabank.ru/metrica/corp".to_string(),
            platform: "web".to_string(),
            tracker_version: "atlas-0.3.1".to_string(),
            trusted_cert_pem: None,
        }
    }

    fn event() -> MetricEvent {
        MetricEvent {
            category: "ALFA-ATLAS > App".to_string(),
            action: "Install -> First launch".to_string(),
            label: "App installed".to_string(),
            property: None,
            value: None,
            dimensions: BTreeMap::from([
                (SLOT_INSTALL_ID.to_string(), "install-uuid".to_string()),
                (SLOT_APP_VERSION.to_string(), "0.3.1".to_string()),
                (SLOT_OS.to_string(), "macos".to_string()),
            ]),
        }
    }

    fn payload() -> Value {
        build_payload(&config(), &event(), "install-uuid", "event-uuid", 1_756_400_000_000)
    }

    #[test]
    fn payload_uses_the_tracker_protocol_envelope() {
        let payload = payload();
        assert_eq!(payload["schema"], PAYLOAD_DATA_SCHEMA);
        assert_eq!(payload["data"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn payload_carries_every_required_struct_event_field() {
        let payload = payload();
        let fields = &payload["data"][0];
        assert_eq!(fields["e"], "se");
        assert_eq!(fields["se_ca"], "ALFA-ATLAS > App");
        assert_eq!(fields["se_ac"], "Install -> First launch");
        assert_eq!(fields["se_la"], "App installed");
        assert_eq!(fields["aid"], "alfa-atlas-ui_dev");
        assert_eq!(fields["p"], "web");
        assert_eq!(fields["tv"], "atlas-0.3.1");
        assert_eq!(fields["eid"], "event-uuid");
        assert_eq!(fields["dtm"], "1756400000000");
        assert_eq!(fields["duid"], "install-uuid");
    }

    #[test]
    fn every_payload_field_is_a_string() {
        let payload = payload();
        for (key, value) in payload["data"][0].as_object().unwrap() {
            assert!(
                value.is_string(),
                "field {key} must be a string, got {value}"
            );
        }
    }

    #[test]
    fn dimensions_travel_in_a_custom_dimension_context() {
        let payload = payload();
        let contexts: Value =
            serde_json::from_str(payload["data"][0]["co"].as_str().unwrap()).unwrap();
        assert_eq!(contexts["schema"], CONTEXTS_SCHEMA);
        assert_eq!(contexts["data"][0]["schema"], CUSTOM_DIMENSION_SCHEMA);
        assert_eq!(contexts["data"][0]["data"]["2"], "install-uuid");
        assert_eq!(contexts["data"][0]["data"]["3"], "0.3.1");
        assert_eq!(contexts["data"][0]["data"]["4"], "macos");
    }

    #[test]
    fn an_event_without_dimensions_omits_the_context_field() {
        let bare = MetricEvent {
            dimensions: BTreeMap::new(),
            ..event()
        };
        let payload = build_payload(&config(), &bare, "install-uuid", "event-uuid", 0);
        assert!(payload["data"][0].get("co").is_none());
    }

    /// Opt-in live smoke test against the real collector:
    /// `cargo test --lib -- --ignored live_collector`.
    /// Requires the corporate network. Ignored by default so CI and a
    /// developer off the VPN never depend on it.
    /// Prints a copy-pasteable curl for a manual probe event, built by
    /// the real `build_payload` so the shape cannot drift from what the
    /// app actually sends:
    /// `cargo test --lib -- --ignored --nocapture print_probe_curl`
    #[test]
    #[ignore]
    fn print_probe_curl() {
        let probe = MetricEvent {
            category: "ALFA-ATLAS > Debug".to_string(),
            action: "Probe -> Manual check".to_string(),
            label: "Manual collector probe".to_string(),
            property: None,
            value: None,
            // Every registered slot, so the printed curl cannot drift
            // from the shape the app actually sends.
            dimensions: BTreeMap::from([
                (SLOT_INSTALL_ID.to_string(), "PROBE_INSTALL_ID".to_string()),
                (SLOT_APP_VERSION.to_string(), env!("CARGO_PKG_VERSION").to_string()),
                (SLOT_OS.to_string(), std::env::consts::OS.to_string()),
            ]),
        };
        let cfg = crate::domain::metrics::MetricsConfig::resolve()
            .expect("this build ships a metrics section");
        let payload = build_payload(&cfg, &probe, "PROBE_INSTALL_ID", "EVENT_ID", 0);
        println!("### {}", serde_json::to_string(&payload).unwrap());
        println!("### url {}/com.snowplowanalytics.snowplow/tp2", cfg.collector_base);
    }

    #[test]
    #[ignore]
    fn live_collector_accepts_the_install_payload() {
        // The real bundled config, so this also proves the manifest's
        // trust root actually verifies the collector's chain.
        let config = crate::domain::metrics::MetricsConfig::resolve()
            .expect("this build ships a metrics section");
        let result = send(&config, &event(), "smoke-test-install-id");
        assert!(result.is_ok(), "collector rejected the payload: {result:?}");
    }

    #[test]
    fn optional_property_and_value_are_only_sent_when_present() {
        let bare = payload();
        assert!(bare["data"][0].get("se_pr").is_none());
        assert!(bare["data"][0].get("se_va").is_none());

        let filled = MetricEvent {
            property: Some("prop".to_string()),
            value: Some(12.0),
            ..event()
        };
        let payload = build_payload(&config(), &filled, "install-uuid", "event-uuid", 0);
        assert_eq!(payload["data"][0]["se_pr"], "prop");
        assert_eq!(payload["data"][0]["se_va"], "12");
    }
}
