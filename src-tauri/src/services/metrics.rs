//! Orchestrates product metrics: the enabled check, the install event and
//! its exactly-once guarantee.
//!
//! Best-effort throughout, on the same contract as `infra::tool_call_log`:
//! a metrics failure must never break, block or slow down anything the
//! user asked for.

use std::collections::BTreeMap;

use crate::domain::metrics::{
    MetricEvent, MetricsConfig, MetricsError, MetricsState, SLOT_APP_VERSION, SLOT_INSTALL_ID,
    SLOT_OS,
};
use crate::infra::{metrics_store, snowplow_client};

pub const APP_CATEGORY: &str = "ALFA-ATLAS > App";

/// `macos`, `windows`, `linux`, … — `std::env::consts::OS` verbatim, so
/// the value is stable across releases and matches what every other Rust
/// tool reports. Deliberately just the OS family: no version, build number
/// or architecture, none of which is needed to answer "which platforms is
/// this used on" and each of which narrows the anonymity set.
fn os_name() -> &'static str {
    std::env::consts::OS
}

fn install_event(install_id: &str) -> MetricEvent {
    MetricEvent {
        category: APP_CATEGORY.to_string(),
        action: "Install -> First launch".to_string(),
        label: "App installed".to_string(),
        property: None,
        value: None,
        dimensions: BTreeMap::from([
            (SLOT_INSTALL_ID.to_string(), install_id.to_string()),
            (
                SLOT_APP_VERSION.to_string(),
                env!("CARGO_PKG_VERSION").to_string(),
            ),
            (SLOT_OS.to_string(), os_name().to_string()),
        ]),
    }
}

/// Sends one event, or does nothing if the user has metrics off.
pub fn track(event: MetricEvent) -> Result<(), MetricsError> {
    let Some(config) = MetricsConfig::resolve() else {
        return Ok(());
    };
    track_with(event, &config, snowplow_client::send)
}

/// Generic over the transport so tests can substitute one that fails or
/// records instead of reaching the network.
fn track_with<F>(event: MetricEvent, config: &MetricsConfig, send: F) -> Result<(), MetricsError>
where
    F: Fn(&MetricsConfig, &MetricEvent, &str) -> Result<(), MetricsError>,
{
    let (state, install_id) = metrics_store::ensure_install_id()?;
    if !state.enabled {
        return Ok(());
    }
    send(config, &event, &install_id)
}

/// Reports the install event at most once per `~/.atlas` profile.
///
/// `install_reported_at` is written **only** after the collector confirms
/// delivery. That single ordering is what gives this the offline behaviour
/// the kit lacks: a launch outside the corporate network fails the send,
/// leaves the flag unset, and the next launch retries. It also means a
/// user who never gets on the VPN is never counted, which is the honest
/// outcome — the alternative is recording an install that no dashboard
/// ever saw.
///
/// Returns whether the event was actually sent on this call.
pub fn report_install_once() -> Result<bool, MetricsError> {
    // No manifest section means this build ships without telemetry — not
    // an error, just nothing to do.
    let Some(config) = MetricsConfig::resolve() else {
        return Ok(false);
    };
    report_install_once_with(&config, snowplow_client::send)
}

fn report_install_once_with<F>(config: &MetricsConfig, send: F) -> Result<bool, MetricsError>
where
    F: Fn(&MetricsConfig, &MetricEvent, &str) -> Result<(), MetricsError>,
{
    let state = metrics_store::load()?;
    if !state.enabled || state.install_reported_at.is_some() {
        return Ok(false);
    }

    let (mut state, install_id) = metrics_store::ensure_install_id()?;
    send(config, &install_event(&install_id), &install_id)?;

    state.install_reported_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    );
    metrics_store::save(&state)?;
    Ok(true)
}

pub fn set_enabled(enabled: bool) -> Result<MetricsState, MetricsError> {
    let mut state = metrics_store::load()?;
    state.enabled = enabled;
    metrics_store::save(&state)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;
    use std::cell::RefCell;

    fn config() -> MetricsConfig {
        MetricsConfig {
            app_id: "alfa-atlas-ui_dev".to_string(),
            collector_base: "https://example.invalid/metrica/corp".to_string(),
            platform: "web".to_string(),
            tracker_version: "atlas-test".to_string(),
            trusted_cert_pem: None,
        }
    }

    /// Records what the transport was asked to send, and whether it should
    /// succeed. `RefCell` rather than a lock: `with_temp_home` already
    /// serializes these tests against each other.
    struct SpySender {
        succeed: bool,
        sent: RefCell<Vec<MetricEvent>>,
    }

    impl SpySender {
        fn new(succeed: bool) -> Self {
            Self {
                succeed,
                sent: RefCell::new(Vec::new()),
            }
        }

        fn sender(&self) -> impl Fn(&MetricsConfig, &MetricEvent, &str) -> Result<(), MetricsError> + '_ {
            move |_config, event, _install_id| {
                self.sent.borrow_mut().push(event.clone());
                if self.succeed {
                    Ok(())
                } else {
                    Err(MetricsError::Transport("network unreachable".to_string()))
                }
            }
        }

        fn count(&self) -> usize {
            self.sent.borrow().len()
        }
    }

    #[test]
    fn a_confirmed_send_marks_the_install_reported() {
        with_temp_home(|| {
            let spy = SpySender::new(true);
            let sent = report_install_once_with(&config(), spy.sender()).unwrap();

            assert!(sent);
            assert_eq!(spy.count(), 1);
            assert!(metrics_store::load().unwrap().install_reported_at.is_some());
        });
    }

    #[test]
    fn a_failed_send_leaves_the_install_unreported_so_the_next_launch_retries() {
        with_temp_home(|| {
            let spy = SpySender::new(false);
            let error = report_install_once_with(&config(), spy.sender()).unwrap_err();

            assert!(matches!(error, MetricsError::Transport(_)));
            assert_eq!(spy.count(), 1);
            let state = metrics_store::load().unwrap();
            assert!(
                state.install_reported_at.is_none(),
                "an unconfirmed send must not count as reported"
            );
            // The id survives the failure, so the retry reports the same
            // installation rather than inventing a second one.
            assert!(state.install_id.is_some());

            let retry = SpySender::new(true);
            assert!(report_install_once_with(&config(), retry.sender()).unwrap());
            assert_eq!(
                retry.sent.borrow()[0].dimensions[SLOT_INSTALL_ID],
                state.install_id.unwrap()
            );
        });
    }

    #[test]
    fn a_second_call_after_a_confirmed_send_is_a_no_op() {
        with_temp_home(|| {
            let spy = SpySender::new(true);
            assert!(report_install_once_with(&config(), spy.sender()).unwrap());
            assert!(!report_install_once_with(&config(), spy.sender()).unwrap());
            assert_eq!(spy.count(), 1, "the install event must be sent exactly once");
        });
    }

    #[test]
    fn nothing_is_sent_while_metrics_are_disabled() {
        with_temp_home(|| {
            set_enabled(false).unwrap();
            let spy = SpySender::new(true);

            assert!(!report_install_once_with(&config(), spy.sender()).unwrap());
            track_with(install_event("id"), &config(), spy.sender()).unwrap();

            assert_eq!(spy.count(), 0);
            assert!(metrics_store::load().unwrap().install_reported_at.is_none());
        });
    }

    #[test]
    fn the_install_event_carries_the_install_id_and_app_version() {
        with_temp_home(|| {
            let spy = SpySender::new(true);
            report_install_once_with(&config(), spy.sender()).unwrap();

            let event = spy.sent.borrow()[0].clone();
            assert_eq!(event.category, APP_CATEGORY);
            assert_eq!(event.action, "Install -> First launch");
            assert_eq!(
                event.dimensions[SLOT_INSTALL_ID],
                metrics_store::load().unwrap().install_id.unwrap()
            );
            assert_eq!(event.dimensions[SLOT_APP_VERSION], env!("CARGO_PKG_VERSION"));
        });
    }

    #[test]
    fn the_install_event_carries_the_operating_system() {
        with_temp_home(|| {
            let spy = SpySender::new(true);
            report_install_once_with(&config(), spy.sender()).unwrap();

            let os = spy.sent.borrow()[0].dimensions[SLOT_OS].clone();
            assert_eq!(os, std::env::consts::OS);
            assert!(
                ["macos", "windows", "linux"].contains(&os.as_str()),
                "unexpected os value {os} — a new target needs a look at the slot-4 registry"
            );
        });
    }
}
