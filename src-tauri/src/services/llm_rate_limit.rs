//! Application-layer store for LLM rate-limit usage events.
//!
//! Owns the event log (in-memory + `~/.atlas/llm-rate-limit.json`) and
//! delegates snapshot math to [`crate::domain::llm_rate_limit::RateLimitPolicy`].
//! Callers record completion tokens after each successful HTTP round; the
//! UI polls [`snapshot`] for the status-bar chip.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::domain::llm_rate_limit::{policy_for, RateLimitSnapshot, UsageEvent};
use crate::infra::llm_provider_manifest;
use crate::infra::settings_store;
use crate::services::llm_config;

const STORE_FILE_NAME: &str = "llm-rate-limit.json";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedStore {
    /// Monotonic id generator so samples stay stable across reloads.
    next_id: u64,
    /// Keyed by provider id. Only providers with a non-noop policy keep entries.
    #[serde(default)]
    providers: std::collections::HashMap<String, Vec<UsageEvent>>,
}

static STORE: Mutex<Option<PersistedStore>> = Mutex::new(None);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn store_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(STORE_FILE_NAME))
}

fn load_unlocked() -> PersistedStore {
    let Ok(path) = store_path() else {
        return PersistedStore::default();
    };
    if !path.exists() {
        return PersistedStore::default();
    }
    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => PersistedStore::default(),
    }
}

fn save_unlocked(store: &PersistedStore) {
    let Ok(dir) = settings_store::settings_dir() else {
        return;
    };
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(STORE_FILE_NAME);
    if let Ok(contents) = serde_json::to_string_pretty(store) {
        let _ = fs::write(path, contents);
    }
}

fn with_store<T>(f: impl FnOnce(&mut PersistedStore) -> T) -> T {
    let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(load_unlocked());
    }
    f(guard.as_mut().expect("store just initialized"))
}

fn prune(events: &mut Vec<UsageEvent>, retention_ms: Option<i64>, now: i64) {
    let Some(window) = retention_ms else {
        events.clear();
        return;
    };
    let cutoff = now - window;
    events.retain(|e| e.at_ms > cutoff);
}

fn active_policy(provider_id: &str) -> Box<dyn crate::domain::llm_rate_limit::RateLimitPolicy> {
    let settings = llm_config::load_llm_settings().ok();
    let enabled = settings.as_ref().map(|s| s.rate_limit_enabled).unwrap_or(true);
    let enforce_off_hours = settings
        .as_ref()
        .map(|s| s.rate_limit_off_hours_enforced)
        .unwrap_or(false);
    if !enabled {
        return policy_for(None, false);
    }
    policy_for(
        llm_provider_manifest::find_rate_limit(provider_id),
        enforce_off_hours,
    )
}

/// Record one successful LLM HTTP round: its prompt and completion tokens,
/// and — implicitly, by existing at all — one request against the request
/// cap. That last part is why a zero-token round is still recorded.
///
/// No-op when tracking is disabled or the provider has no baked-in rule.
pub fn record(provider_id: &str, prompt_tokens: u32, completion_tokens: u32) {
    let policy = active_policy(provider_id);
    let Some(retention) = policy.retention_ms() else {
        return;
    };
    let now = now_ms();
    with_store(|store| {
        let events = store.providers.entry(provider_id.to_string()).or_default();
        prune(events, Some(retention), now);
        let id = store.next_id;
        store.next_id = store.next_id.saturating_add(1);
        events.push(UsageEvent {
            id,
            at_ms: now,
            tokens: completion_tokens,
            prompt_tokens,
        });
        save_unlocked(store);
    });
}

/// Current snapshot for the status-bar chip. Always succeeds — a missing
/// store, disabled tracking, or unknown provider yields the noop snapshot.
pub fn snapshot(provider_id: &str) -> RateLimitSnapshot {
    let policy = active_policy(provider_id);
    let now = now_ms();
    with_store(|store| {
        let events = store.providers.entry(provider_id.to_string()).or_default();
        // Only prune (which clears everything when retention is `None`,
        // i.e. tracking disabled or no baked-in policy) when there's a
        // real retention window — a read while disabled must not destroy
        // the accumulated log that a re-enable will want back.
        if let Some(retention) = policy.retention_ms() {
            prune(events, Some(retention), now);
            let snap = policy.snapshot(events, now);
            save_unlocked(store);
            snap
        } else {
            policy.snapshot(events, now)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;

    use crate::domain::llm_rate_limit::RateLimitResourceKind;

    fn used(snap: &crate::domain::llm_rate_limit::RateLimitSnapshot, kind: RateLimitResourceKind) -> u32 {
        snap.resources
            .iter()
            .find(|r| r.kind == kind)
            .map(|r| r.used)
            .unwrap_or(0)
    }

    #[test]
    fn record_and_snapshot_round_trip() {
        with_temp_home(|| {
            // Reset in-memory cache so this test's HOME is what we load.
            *STORE.lock().unwrap() = None;
            record("alfagen", 300_000, 12_000);
            let snap = snapshot("alfagen");
            assert_eq!(snap.policy_id, "evc-sliding-window");
            assert_eq!(used(&snap, RateLimitResourceKind::Completion), 12_000);
            assert_eq!(used(&snap, RateLimitResourceKind::Prompt), 300_000);
            assert_eq!(used(&snap, RateLimitResourceKind::Requests), 1);
            assert_eq!(snap.samples.len(), 1);
            // Reload from disk through a fresh cache.
            *STORE.lock().unwrap() = None;
            let snap2 = snapshot("alfagen");
            assert_eq!(used(&snap2, RateLimitResourceKind::Completion), 12_000);
            assert_eq!(used(&snap2, RateLimitResourceKind::Prompt), 300_000);
        });
    }

    #[test]
    fn a_token_free_round_still_counts_as_a_request() {
        with_temp_home(|| {
            *STORE.lock().unwrap() = None;
            record("alfagen", 0, 0);
            let snap = snapshot("alfagen");
            assert_eq!(used(&snap, RateLimitResourceKind::Requests), 1);
        });
    }

    #[test]
    fn noop_provider_does_not_persist() {
        with_temp_home(|| {
            *STORE.lock().unwrap() = None;
            record("custom-openai", 10_000, 50_000);
            let snap = snapshot("custom-openai");
            assert_eq!(snap.policy_id, "none");
            assert_eq!(snap.used, 0);
            let path = store_path().unwrap();
            // File may or may not exist; if it does, provider key should be absent.
            if path.exists() {
                let raw = fs::read_to_string(&path).unwrap();
                assert!(!raw.contains("custom-openai"));
            }
        });
    }

    #[test]
    fn disabled_in_settings_is_noop_even_for_alfagen() {
        with_temp_home(|| {
            *STORE.lock().unwrap() = None;
            let settings = crate::domain::llm::LlmSettings {
                rate_limit_enabled: false,
                ..Default::default()
            };
            llm_config::save_llm_settings(settings).unwrap();
            record("alfagen", 300_000, 12_000);
            let snap = snapshot("alfagen");
            assert_eq!(snap.policy_id, "none");
            assert_eq!(snap.used, 0);
            assert!(snap.resources.is_empty());
        });
    }

    #[test]
    fn off_hours_setting_reaches_the_policy() {
        with_temp_home(|| {
            *STORE.lock().unwrap() = None;
            let settings = crate::domain::llm::LlmSettings {
                rate_limit_off_hours_enforced: true,
                ..Default::default()
            };
            llm_config::save_llm_settings(settings).unwrap();
            record("alfagen", 1_000, 100);
            // Whatever the clock says right now, the override means the
            // window is being counted — never the off-hours severity.
            let snap = snapshot("alfagen");
            assert!(snap.is_enforced);
            assert_ne!(
                snap.severity,
                crate::domain::llm_rate_limit::RateLimitSeverity::OffHours
            );
        });
    }
}
