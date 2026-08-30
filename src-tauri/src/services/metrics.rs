//! Orchestrates product metrics: the enabled check, the slots stamped on
//! every event, delivery through the disk queue, and the install event's
//! exactly-once guarantee.
//!
//! Best-effort throughout, on the same contract as `infra::tool_call_log`:
//! a metrics failure must never break, block or slow down anything the
//! user asked for.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde_json::Value;
use uuid::Uuid;

use crate::domain::metrics::{
    MetricEvent, MetricsConfig, MetricsError, MetricsState, PROVIDER_CUSTOM, SLOT_APP_VERSION,
    SLOT_INSTALL_ID, SLOT_OS, SLOT_PROVIDER, SLOT_SESSION_ID,
};
use crate::infra::{metrics_queue, metrics_store, snowplow_client};
use crate::services::metrics_session;

pub const APP_CATEGORY: &str = "ALFA-ATLAS > App";

/// `macos`, `windows`, `linux`, … — `std::env::consts::OS` verbatim, so
/// the value is stable across releases and matches what every other Rust
/// tool reports. Deliberately just the OS family: no version, build number
/// or architecture, none of which is needed to answer "which platforms is
/// this used on" and each of which narrows the anonymity set.
fn os_name() -> &'static str {
    std::env::consts::OS
}

/// Slots every event carries, stamped here rather than by each call site.
/// These are the cross-cutting dimensions — the ones dashboards slice
/// *all* categories by. Event-specific detail belongs in `label` /
/// `property` / `value`, which are free-form and cost no slot.
fn base_dimensions(install_id: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (SLOT_INSTALL_ID.to_string(), install_id.to_string()),
        (
            SLOT_APP_VERSION.to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        ),
        (SLOT_OS.to_string(), os_name().to_string()),
        (
            SLOT_SESSION_ID.to_string(),
            metrics_session::id().to_string(),
        ),
    ])
}

/// Replaces any dimension value that could carry something identifying
/// with a safe stand-in, at the boundary rather than at the call site.
///
/// Today that is slot 6: a provider id is free text the user typed when
/// they configured their own endpoint, and has been seen in the wild to
/// contain internal hostnames. Only ids that exist in the bundled manifest
/// are real names; everything else becomes `custom`. Enforcing it here
/// means no call site — present or future, frontend or Rust — can leak one
/// by forgetting.
fn sanitize_dimensions(dimensions: &mut BTreeMap<String, String>) {
    if let Some(provider) = dimensions.get(SLOT_PROVIDER) {
        if crate::infra::llm_provider_manifest::find_system_provider(provider).is_none() {
            dimensions.insert(SLOT_PROVIDER.to_string(), PROVIDER_CUSTOM.to_string());
        }
    }
}

/// Merges the base slots in without letting them be overwritten: a call
/// site that accidentally reuses slot 2 must not be able to replace the
/// install id with something else.
fn with_base_dimensions(mut event: MetricEvent, install_id: &str) -> MetricEvent {
    let mut dimensions = event.dimensions;
    sanitize_dimensions(&mut dimensions);
    dimensions.extend(base_dimensions(install_id));
    event.dimensions = dimensions;
    event
}

pub fn app_start_event() -> MetricEvent {
    MetricEvent {
        category: APP_CATEGORY.to_string(),
        action: "Start -> App".to_string(),
        label: "App started".to_string(),
        property: None,
        value: None,
        dimensions: BTreeMap::new(),
    }
}

/// `value` is **active** seconds — time the window actually had focus —
/// because a docs tool left open behind a browser all day is not an
/// eight-hour session in any sense that matters. Wall-clock time from
/// launch to exit rides along in `property` so the two can be compared:
/// the ratio between them is itself the answer to "is this app used, or
/// merely open".
///
/// Both are whole seconds, not minutes: a large share of desktop sessions
/// are short, and rounding those to a minute would erase the difference
/// between "opened and immediately closed" and "worked for a while".
pub fn session_end_event(active_seconds: f64, total_seconds: f64) -> MetricEvent {
    MetricEvent {
        category: APP_CATEGORY.to_string(),
        action: "End -> Session".to_string(),
        label: "Session ended".to_string(),
        property: Some(format!("{}", total_seconds.round())),
        value: Some(active_seconds.round()),
        dimensions: BTreeMap::new(),
    }
}

fn install_event() -> MetricEvent {
    MetricEvent {
        category: APP_CATEGORY.to_string(),
        action: "Install -> First launch".to_string(),
        label: "App installed".to_string(),
        property: None,
        value: None,
        dimensions: BTreeMap::new(),
    }
}

/// Queues one event and opportunistically tries to drain the queue.
///
/// Returns `Ok` as soon as the event is safely on disk. A failed flush is
/// not a tracking failure — that is the entire point of the queue, and
/// surfacing it would turn "you are off the VPN" into an error in a flow
/// the user actually asked for.
pub fn track(event: MetricEvent) -> Result<(), MetricsError> {
    let Some(config) = MetricsConfig::resolve() else {
        return Ok(());
    };
    enqueue_event(&config, event)?;
    // The event is already safe on disk, so a failed drain is not a
    // tracking failure — but it is never silent: a queue that quietly
    // stops draining is the one failure mode that would go unnoticed
    // until someone asks why the dashboards are empty.
    if let Err(e) = flush() {
        eprintln!("metrics: queue flush failed: {e}");
    }
    Ok(())
}

/// Puts one event on disk. Split from `track` so it can be exercised
/// without the opportunistic flush — and therefore without a network.
fn enqueue_event(config: &MetricsConfig, event: MetricEvent) -> Result<(), MetricsError> {
    let (state, install_id) = metrics_store::ensure_install_id()?;
    if !state.enabled {
        return Ok(());
    }

    let fields = snowplow_client::build_event_fields(
        config,
        &with_base_dimensions(event, &install_id),
        &install_id,
        &Uuid::new_v4().to_string(),
        snowplow_client::unix_millis(),
    );
    metrics_queue::enqueue(&fields, snowplow_client::unix_millis())
        .map_err(|e| MetricsError::Transport(e.to_string()))
}

/// Queues one event without trying to send it.
///
/// For shutdown: the app is about to exit, so a network round trip would
/// either delay the exit or be killed halfway. A local SQLite insert takes
/// milliseconds and cannot be lost — the next launch's flush carries it
/// out.
pub fn track_deferred(event: MetricEvent) -> Result<(), MetricsError> {
    let Some(config) = MetricsConfig::resolve() else {
        return Ok(());
    };
    enqueue_event(&config, event)
}

/// Held for the duration of a flush so two concurrent drains can't post —
/// and then delete — the same rows twice. A flush that can't take the lock
/// simply skips: another one is already doing the work, and the holder
/// drains until empty, so the skipped caller's event still goes out.
static FLUSH_LOCK: Mutex<()> = Mutex::new(());

/// Bounds the drain loop. At `FLUSH_BATCH` events per round this is far
/// more than any real backlog, and it guarantees the loop ends even if
/// deletion silently fails and the same rows keep coming back.
const MAX_FLUSH_ROUNDS: usize = 32;

/// Sends everything pending, oldest first. Returns how many events the
/// collector confirmed.
pub fn flush() -> Result<usize, MetricsError> {
    let Some(config) = MetricsConfig::resolve() else {
        return Ok(0);
    };
    flush_with(&config, snowplow_client::post_payload)
}

fn flush_with<F>(config: &MetricsConfig, post: F) -> Result<usize, MetricsError>
where
    F: Fn(&MetricsConfig, &Value) -> Result<(), MetricsError>,
{
    let Ok(_guard) = FLUSH_LOCK.try_lock() else {
        return Ok(0);
    };

    // Drains until the queue is empty rather than sending a single batch.
    // Without the loop, an event enqueued while this flush was already in
    // flight would sit untouched until something else happened to trigger
    // a flush — its own attempt having been skipped by the lock above.
    // That stranded exactly the last event of every session.
    let mut confirmed = 0;
    for _ in 0..MAX_FLUSH_ROUNDS {
        let batch =
            metrics_queue::take_batch().map_err(|e| MetricsError::Transport(e.to_string()))?;
        if batch.is_empty() {
            break;
        }

        // `stm` is stamped now, not at enqueue time: a queued event can be
        // days old, and the gap between `dtm` and `stm` is how the
        // collector corrects for it.
        let sent_at = snowplow_client::unix_millis().to_string();
        let (ids, events): (Vec<i64>, Vec<Value>) = batch
            .into_iter()
            .map(|(id, mut fields)| {
                if let Some(object) = fields.as_object_mut() {
                    object.insert("stm".to_string(), Value::String(sent_at.clone()));
                }
                (id, fields)
            })
            .unzip();

        post(config, &snowplow_client::wrap_payload(&events))?;

        metrics_queue::delete_confirmed(&ids)
            .map_err(|e| MetricsError::Transport(e.to_string()))?;
        confirmed += ids.len();
    }
    Ok(confirmed)
}

/// Reports the install event at most once per `~/.atlas` profile.
///
/// Deliberately **not** routed through the queue: the exactly-once
/// guarantee rests on `install_reported_at` being written only after the
/// collector confirms delivery, and enqueueing confirms nothing. A failed
/// send leaves the flag unset and the next launch retries — which also
/// means a user who never reaches the network is never counted, the honest
/// outcome versus recording an install no dashboard ever saw.
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
    send(
        config,
        &with_base_dimensions(install_event(), &install_id),
        &install_id,
    )?;

    state.install_reported_at = Some(snowplow_client::unix_millis());
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

        fn sender(
            &self,
        ) -> impl Fn(&MetricsConfig, &MetricEvent, &str) -> Result<(), MetricsError> + '_ {
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

    struct SpyPoster {
        succeed: bool,
        posted: RefCell<Vec<Value>>,
    }

    impl SpyPoster {
        fn new(succeed: bool) -> Self {
            Self {
                succeed,
                posted: RefCell::new(Vec::new()),
            }
        }

        fn poster(&self) -> impl Fn(&MetricsConfig, &Value) -> Result<(), MetricsError> + '_ {
            move |_config, payload| {
                self.posted.borrow_mut().push(payload.clone());
                if self.succeed {
                    Ok(())
                } else {
                    Err(MetricsError::Transport("network unreachable".to_string()))
                }
            }
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
            enqueue_event(&config(), app_start_event()).unwrap();

            assert_eq!(spy.count(), 0);
            assert_eq!(metrics_queue::pending_count().unwrap(), 0);
            assert!(metrics_store::load().unwrap().install_reported_at.is_none());
        });
    }

    #[test]
    fn every_event_carries_the_cross_cutting_slots() {
        with_temp_home(|| {
            let spy = SpySender::new(true);
            report_install_once_with(&config(), spy.sender()).unwrap();

            let event = spy.sent.borrow()[0].clone();
            assert_eq!(event.category, APP_CATEGORY);
            assert_eq!(
                event.dimensions[SLOT_INSTALL_ID],
                metrics_store::load().unwrap().install_id.unwrap()
            );
            assert_eq!(event.dimensions[SLOT_APP_VERSION], env!("CARGO_PKG_VERSION"));
            assert_eq!(event.dimensions[SLOT_OS], std::env::consts::OS);
            assert_eq!(event.dimensions[SLOT_SESSION_ID], metrics_session::id());
        });
    }

    #[test]
    fn a_user_configured_provider_name_never_leaves_the_machine() {
        let event = MetricEvent {
            dimensions: BTreeMap::from([(
                SLOT_PROVIDER.to_string(),
                "eugene-llm.internal.corp".to_string(),
            )]),
            ..app_start_event()
        };
        let event = with_base_dimensions(event, "install-id");
        assert_eq!(event.dimensions[SLOT_PROVIDER], PROVIDER_CUSTOM);
    }

    #[test]
    fn a_bundled_provider_is_reported_under_its_real_id() {
        let known = crate::infra::llm_provider_manifest::system_providers()
            .first()
            .expect("this build ships at least one system provider")
            .id
            .clone();
        let event = MetricEvent {
            dimensions: BTreeMap::from([(SLOT_PROVIDER.to_string(), known.clone())]),
            ..app_start_event()
        };
        let event = with_base_dimensions(event, "install-id");
        assert_eq!(event.dimensions[SLOT_PROVIDER], known);
    }

    #[test]
    fn a_call_site_cannot_overwrite_a_base_slot() {
        let hijacked = MetricEvent {
            dimensions: BTreeMap::from([(SLOT_INSTALL_ID.to_string(), "spoofed".to_string())]),
            ..app_start_event()
        };
        let event = with_base_dimensions(hijacked, "real-install-id");
        assert_eq!(event.dimensions[SLOT_INSTALL_ID], "real-install-id");
    }

    #[test]
    fn session_end_reports_active_time_as_the_value_and_total_alongside() {
        let event = session_end_event(42.4, 900.0);
        assert_eq!(
            event.value,
            Some(42.0),
            "the headline number must be focused time, not time merely open"
        );
        assert_eq!(event.property.as_deref(), Some("900"));
    }

    #[test]
    fn a_short_session_does_not_round_away_to_nothing() {
        assert_eq!(session_end_event(20.0, 20.0).value, Some(20.0));
    }

    #[test]
    fn a_tracked_event_is_queued_and_then_drained() {
        with_temp_home(|| {
            enqueue_event(&config(), app_start_event()).unwrap();
            assert_eq!(metrics_queue::pending_count().unwrap(), 1);

            let spy = SpyPoster::new(true);
            assert_eq!(flush_with(&config(), spy.poster()).unwrap(), 1);
            assert_eq!(metrics_queue::pending_count().unwrap(), 0);

            let payload = spy.posted.borrow()[0].clone();
            assert_eq!(payload["data"].as_array().unwrap().len(), 1);
            assert_eq!(payload["data"][0]["se_ac"], "Start -> App");
            assert!(
                payload["data"][0]["stm"].is_string(),
                "a queued event must be stamped with its send time"
            );
        });
    }

    #[test]
    fn a_failed_flush_keeps_every_event_for_the_next_attempt() {
        with_temp_home(|| {
            enqueue_event(&config(), app_start_event()).unwrap();
            enqueue_event(&config(), session_end_event(10.0, 30.0)).unwrap();

            let spy = SpyPoster::new(false);
            assert!(flush_with(&config(), spy.poster()).is_err());
            assert_eq!(
                metrics_queue::pending_count().unwrap(),
                2,
                "nothing may be dropped when the collector never confirmed"
            );
        });
    }

    /// Opt-in: pushes one event of every wave-2 shape through the real
    /// queue to the real collector, so the payload — including slot 6 —
    /// is validated against what actually accepts it.
    /// `cargo test --lib -- --ignored --nocapture live_assistant`
    #[test]
    #[ignore]
    fn live_assistant_events_are_accepted() {
        let config = MetricsConfig::resolve().expect("a metrics section");
        let provider = crate::infra::llm_provider_manifest::system_providers()
            .first()
            .expect("a system provider")
            .id
            .clone();

        let shapes: Vec<(&str, &str, Option<String>, Option<f64>, String)> = vec![
            ("Settle -> Turn", "done", Some("docsOnly".into()), Some(12.0), provider.clone()),
            ("Fail -> Turn", "rateLimit", None, None, provider.clone()),
            ("Decide -> Tool calls", "approved", None, Some(3.0), provider.clone()),
            ("Run -> Tool", "ok", Some("readFile".into()), Some(30.0), provider.clone()),
            ("Compact -> Context", "auto", None, Some(8.0), provider.clone()),
            ("Switch -> Access mode", "user", Some("fullRepo".into()), None, provider.clone()),
            ("Switch -> Access mode", "assistant-granted", Some("fullRepo".into()), None, provider.clone()),
            ("Switch -> Access mode", "assistant-denied", Some("fullRepo".into()), None, provider.clone()),
            ("Switch -> Conversation mode", "assistant-denied", Some("agent".into()), None, provider.clone()),
            // A user-configured provider must arrive as `custom`.
            ("Settle -> Turn", "error", None, Some(1.0), "eugene-llm.internal".into()),
        ];

        for (action, label, property, value, provider_id) in shapes {
            let event = MetricEvent {
                category: "ALFA-ATLAS > Assistant".to_string(),
                action: action.to_string(),
                label: label.to_string(),
                property,
                value,
                dimensions: BTreeMap::from([(SLOT_PROVIDER.to_string(), provider_id)]),
            };
            enqueue_event(&config, event).expect("enqueue");
        }

        match flush() {
            Ok(n) => println!("collector accepted {n} assistant event(s)"),
            Err(e) => panic!("flush failed: {e}"),
        }
    }

    /// Opt-in end-to-end check of the real queue against the real
    /// collector: `cargo test --lib -- --ignored --nocapture live_flush`.
    /// Requires the corporate network. Runs against the actual `~/.atlas`
    /// profile, so it also reports what is already stranded there.
    #[test]
    #[ignore]
    fn live_flush_drains_the_real_queue() {
        let config = MetricsConfig::resolve().expect("a metrics section");
        println!("collector: {}", config.collector_base);
        enqueue_event(&config, app_start_event()).expect("enqueue");
        match flush() {
            Ok(n) => println!("flushed {n} event(s)"),
            Err(e) => panic!("flush failed: {e}"),
        }
    }

    /// Regression: the last event of a session used to be stranded. Its
    /// own flush was skipped because another flush held the lock, and that
    /// holder had already taken its batch before the event was enqueued —
    /// so nothing carried it out until the next launch.
    #[test]
    fn an_event_enqueued_during_a_flush_still_goes_out() {
        with_temp_home(|| {
            enqueue_event(&config(), app_start_event()).unwrap();

            let posts = RefCell::new(0usize);
            let poster = |_c: &MetricsConfig, _p: &Value| {
                *posts.borrow_mut() += 1;
                // Mid-flight arrival, exactly as a frontend event would.
                if *posts.borrow() == 1 {
                    enqueue_event(&config(), session_end_event(1.0, 2.0)).unwrap();
                }
                Ok(())
            };

            assert_eq!(flush_with(&config(), poster).unwrap(), 2);
            assert_eq!(
                metrics_queue::pending_count().unwrap(),
                0,
                "the event that arrived mid-flush must not be stranded"
            );
        });
    }

    #[test]
    fn a_flush_posts_the_whole_backlog_as_one_batch() {
        with_temp_home(|| {
            enqueue_event(&config(), app_start_event()).unwrap();
            enqueue_event(&config(), session_end_event(5.0, 12.0)).unwrap();
            enqueue_event(&config(), app_start_event()).unwrap();

            let spy = SpyPoster::new(true);
            assert_eq!(flush_with(&config(), spy.poster()).unwrap(), 3);
            assert_eq!(
                spy.posted.borrow().len(),
                1,
                "three events must cost one request, not three"
            );
            assert_eq!(spy.posted.borrow()[0]["data"].as_array().unwrap().len(), 3);
        });
    }
}
