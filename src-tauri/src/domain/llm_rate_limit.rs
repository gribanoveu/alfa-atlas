//! Pluggable LLM rate-limit policies.
//!
//! The algorithm lives behind [`RateLimitPolicy`]; the UI only sees a
//! stable [`RateLimitSnapshot`]. Swapping the formula (sliding window →
//! token bucket, different hours, different caps) means a new
//! implementation + a one-line change in [`policy_for`] — not a UI rewrite.
//!
//! Policies are pure: they do not own the event log and do no I/O.
//! Persistence and recording live in `services::llm_rate_limit`.

use serde::{Deserialize, Serialize};

/// One recorded completion-token spend. `at_ms` is Unix epoch milliseconds
/// (UTC). Policies decide which events fall inside their window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageEvent {
    pub id: u64,
    pub at_ms: i64,
    pub tokens: u32,
}

/// Visual severity for the status-bar chip. Computed by the policy so
/// thresholds (70% / 90%) stay swappable with the algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RateLimitSeverity {
    Normal,
    Warning,
    Critical,
    Limited,
    OffHours,
}

/// One upcoming release of tokens from the sliding window (or equivalent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitRelease {
    /// Epoch ms when these tokens leave the window.
    pub at: i64,
    pub tokens: u32,
}

/// One sample still inside the active window — drives the timeline tab.
/// Policies without a window return an empty list; the UI then hides that tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSample {
    pub id: String,
    pub at: i64,
    pub tokens: u32,
    pub expires_at: i64,
}

/// Stable IPC DTO. The frontend must not hard-code window length, limit, or
/// working hours — everything it needs is here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSnapshot {
    pub policy_id: String,
    pub label: String,
    pub used: u32,
    pub remaining: u32,
    pub limit: u32,
    /// `None` for policies that have no sliding window.
    pub window_ms: Option<i64>,
    pub is_enforced: bool,
    pub is_limited: bool,
    pub severity: RateLimitSeverity,
    pub retry_until: Option<i64>,
    pub next_release_at: Option<i64>,
    pub next_enforce_at: Option<i64>,
    pub releases: Vec<RateLimitRelease>,
    pub samples: Vec<RateLimitSample>,
}

/// Baked-in rate-limit rule from `assets/llm/system_providers.yaml`
/// (`rateLimits` array). A downstream fork edits that file — not this
/// module — to change caps, hours, or which provider is limited.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitPreset {
    pub provider_id: String,
    pub policy_id: String,
    pub label: String,
    pub limit: u32,
    pub window_minutes: u32,
    /// Inclusive local start hour. JSON `null` (or omit) together with
    /// [`Self::work_to_hour`] means the sliding window is enforced 24/7.
    #[serde(default)]
    pub work_from_hour: Option<u8>,
    /// Exclusive local end hour. Both hours must be set to gate by schedule;
    /// either `null` → always on.
    #[serde(default)]
    pub work_to_hour: Option<u8>,
    /// UTC offset in hours (e.g. 3 for Europe/Moscow). No DST. Unused when
    /// working hours are unset.
    pub timezone_offset_hours: i8,
    #[serde(default = "default_warn_ratio")]
    pub warn_ratio: f64,
    #[serde(default = "default_crit_ratio")]
    pub crit_ratio: f64,
}

fn default_warn_ratio() -> f64 {
    0.70
}

fn default_crit_ratio() -> f64 {
    0.90
}

impl RateLimitPreset {
    pub fn window_ms(&self) -> i64 {
        self.window_minutes as i64 * 60 * 1000
    }

    pub fn timezone_offset_ms(&self) -> i64 {
        self.timezone_offset_hours as i64 * 60 * 60 * 1000
    }

    /// `Some((from, to))` when both hours are set; `None` means 24/7.
    pub fn work_hours(&self) -> Option<(u8, u8)> {
        match (self.work_from_hour, self.work_to_hour) {
            (Some(from), Some(to)) => Some((from, to)),
            _ => None,
        }
    }
}

/// Pure rate-limit algorithm. Implementations must be cheap to construct;
/// they hold no mutable state.
pub trait RateLimitPolicy: Send + Sync {
    fn id(&self) -> &str;
    fn label(&self) -> &str;
    fn snapshot(&self, events: &[UsageEvent], now_ms: i64) -> RateLimitSnapshot;
    /// How far back events must be retained for this policy. `None` means
    /// the store can drop everything immediately (noop).
    fn retention_ms(&self) -> Option<i64>;
}

/// Build a policy from a baked-in preset. Unknown `policyId` → noop.
/// Looked up by provider id in `infra::llm_provider_manifest`.
pub fn policy_for(preset: Option<&RateLimitPreset>) -> Box<dyn RateLimitPolicy> {
    match preset {
        Some(p) if p.policy_id == "evc-sliding-window" => {
            Box::new(EvcSlidingWindow { preset: p.clone() })
        }
        _ => Box::new(NoopPolicy),
    }
}

/// Hidden chip: no baked-in rule, unknown policy id, or user disabled tracking.
pub struct NoopPolicy;

impl RateLimitPolicy for NoopPolicy {
    fn id(&self) -> &str {
        "none"
    }

    fn label(&self) -> &str {
        ""
    }

    fn retention_ms(&self) -> Option<i64> {
        None
    }

    fn snapshot(&self, _events: &[UsageEvent], _now_ms: i64) -> RateLimitSnapshot {
        RateLimitSnapshot {
            policy_id: self.id().to_string(),
            label: self.label().to_string(),
            used: 0,
            remaining: 0,
            limit: 0,
            window_ms: None,
            is_enforced: false,
            is_limited: false,
            severity: RateLimitSeverity::OffHours,
            retry_until: None,
            next_release_at: None,
            next_enforce_at: None,
            releases: Vec::new(),
            samples: Vec::new(),
        }
    }
}

/// Sliding window of completion tokens, with optional working-hours gate.
/// Numbers come from [`RateLimitPreset`], not from this struct.
pub struct EvcSlidingWindow {
    preset: RateLimitPreset,
}

impl RateLimitPolicy for EvcSlidingWindow {
    fn id(&self) -> &str {
        &self.preset.policy_id
    }

    fn label(&self) -> &str {
        &self.preset.label
    }

    fn retention_ms(&self) -> Option<i64> {
        Some(self.preset.window_ms())
    }

    fn snapshot(&self, events: &[UsageEvent], now_ms: i64) -> RateLimitSnapshot {
        let window_ms = self.preset.window_ms();
        let offset_ms = self.preset.timezone_offset_ms();
        let cutoff = now_ms - window_ms;
        let mut active: Vec<&UsageEvent> = events.iter().filter(|e| e.at_ms > cutoff).collect();
        active.sort_by_key(|e| e.at_ms);

        let used: u32 = active.iter().map(|e| e.tokens).sum();
        let remaining = self.preset.limit.saturating_sub(used);
        let is_enforced = match self.preset.work_hours() {
            Some((from, to)) => working_hours(now_ms, offset_ms, from, to),
            None => true,
        };
        let is_limited = is_enforced && used >= self.preset.limit;

        let releases: Vec<RateLimitRelease> = active
            .iter()
            .map(|e| RateLimitRelease {
                at: e.at_ms + window_ms,
                tokens: e.tokens,
            })
            .collect();

        let retry_until = if is_limited {
            compute_retry_until(&releases, used, self.preset.limit)
        } else {
            None
        };

        let next_release_at = releases.first().map(|r| r.at);
        let next_enforce_at = match self.preset.work_hours() {
            Some((from, _)) if !is_enforced => Some(next_work_start(now_ms, offset_ms, from)),
            _ => None,
        };

        let samples: Vec<RateLimitSample> = active
            .iter()
            .map(|e| RateLimitSample {
                id: e.id.to_string(),
                at: e.at_ms,
                tokens: e.tokens,
                expires_at: e.at_ms + window_ms,
            })
            .collect();

        let severity = if !is_enforced {
            RateLimitSeverity::OffHours
        } else if is_limited {
            RateLimitSeverity::Limited
        } else {
            let pct = used as f64 / self.preset.limit as f64;
            if pct >= self.preset.crit_ratio {
                RateLimitSeverity::Critical
            } else if pct >= self.preset.warn_ratio {
                RateLimitSeverity::Warning
            } else {
                RateLimitSeverity::Normal
            }
        };

        RateLimitSnapshot {
            policy_id: self.id().to_string(),
            label: self.label().to_string(),
            used,
            remaining,
            limit: self.preset.limit,
            window_ms: Some(window_ms),
            is_enforced,
            is_limited,
            severity,
            retry_until,
            next_release_at,
            next_enforce_at,
            releases,
            samples,
        }
    }
}

/// Earliest moment at which dropping oldest releases brings `used` below `limit`.
fn compute_retry_until(releases: &[RateLimitRelease], used: u32, limit: u32) -> Option<i64> {
    let mut acc = used;
    for r in releases {
        acc = acc.saturating_sub(r.tokens);
        if acc < limit {
            return Some(r.at);
        }
    }
    releases.last().map(|r| r.at)
}

fn working_hours(now_ms: i64, offset_ms: i64, from_hour: u8, to_hour: u8) -> bool {
    let hour = local_hour(now_ms, offset_ms);
    hour >= from_hour as i64 && hour < to_hour as i64
}

fn local_hour(now_ms: i64, offset_ms: i64) -> i64 {
    let local_ms = now_ms + offset_ms;
    let day_ms = 24 * 60 * 60 * 1000;
    let ms_in_day = ((local_ms % day_ms) + day_ms) % day_ms;
    ms_in_day / (60 * 60 * 1000)
}

/// Next `from_hour:00` in the preset timezone strictly after `now_ms`
/// (or today if still before that hour).
fn next_work_start(now_ms: i64, offset_ms: i64, from_hour: u8) -> i64 {
    let local_ms = now_ms + offset_ms;
    let day_ms = 24 * 60 * 60 * 1000;
    let ms_in_day = ((local_ms % day_ms) + day_ms) % day_ms;
    let day_start_local = local_ms - ms_in_day;
    let today_start_local = day_start_local + from_hour as i64 * 60 * 60 * 1000;
    let today_start_utc = today_start_local - offset_ms;
    if now_ms < today_start_utc {
        today_start_utc
    } else {
        today_start_utc + day_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evc_preset() -> RateLimitPreset {
        RateLimitPreset {
            provider_id: "alfagen".to_string(),
            policy_id: "evc-sliding-window".to_string(),
            label: "EVC".to_string(),
            limit: 60_000,
            window_minutes: 30,
            work_from_hour: Some(8),
            work_to_hour: Some(21),
            timezone_offset_hours: 3,
            warn_ratio: 0.70,
            crit_ratio: 0.90,
        }
    }

    fn evc() -> EvcSlidingWindow {
        EvcSlidingWindow { preset: evc_preset() }
    }

    /// Build a UTC epoch ms that is `h:m` in Moscow on 2026-07-15
    /// (Moscow = UTC+3, no DST).
    fn at_moscow(h: i64, m: i64) -> i64 {
        const JUL_15_2026_UTC_MIDNIGHT: i64 = 1_784_073_600_000;
        let msk_midnight = JUL_15_2026_UTC_MIDNIGHT - evc_preset().timezone_offset_ms();
        msk_midnight + h * 3_600_000 + m * 60_000
    }

    fn ev(id: u64, at_ms: i64, tokens: u32) -> UsageEvent {
        UsageEvent { id, at_ms, tokens }
    }

    #[test]
    fn policy_for_evc_preset_vs_none() {
        assert_eq!(policy_for(Some(&evc_preset())).id(), "evc-sliding-window");
        assert_eq!(policy_for(None).id(), "none");
    }

    #[test]
    fn tz_example_limited_retry_in_one_minute() {
        let events = vec![
            ev(1, at_moscow(11, 31), 20_000),
            ev(2, at_moscow(11, 35), 15_000),
            ev(3, at_moscow(11, 40), 15_000),
            ev(4, at_moscow(11, 45), 15_000),
        ];
        let now = at_moscow(12, 0);
        let snap = evc().snapshot(&events, now);
        assert!(snap.is_enforced);
        assert!(snap.is_limited);
        assert_eq!(snap.used, 65_000);
        assert_eq!(snap.severity, RateLimitSeverity::Limited);
        assert_eq!(snap.retry_until, Some(at_moscow(12, 1)));
        assert_eq!(snap.retry_until.unwrap() - now, 60_000);
    }

    #[test]
    fn off_hours_before_eight_not_enforced() {
        let events = vec![ev(1, at_moscow(7, 30), 70_000)];
        let now = at_moscow(7, 50);
        let snap = evc().snapshot(&events, now);
        assert!(!snap.is_enforced);
        assert!(!snap.is_limited);
        assert_eq!(snap.severity, RateLimitSeverity::OffHours);
        assert_eq!(snap.used, 70_000);
        assert_eq!(snap.next_enforce_at, Some(at_moscow(8, 0)));
    }

    #[test]
    fn off_hours_after_twenty_one_not_enforced() {
        let events = vec![ev(1, at_moscow(20, 50), 70_000)];
        let now = at_moscow(21, 5);
        let snap = evc().snapshot(&events, now);
        assert!(!snap.is_enforced);
        assert!(!snap.is_limited);
        assert_eq!(snap.next_enforce_at, Some(at_moscow(8, 0) + 24 * 3_600_000));
    }

    #[test]
    fn only_events_inside_window_count() {
        let now = at_moscow(12, 0);
        let window_ms = evc_preset().window_ms();
        let events = vec![
            ev(1, now - window_ms - 1, 50_000),
            ev(2, now - window_ms + 1, 10_000),
            ev(3, now - 60_000, 5_000),
        ];
        let snap = evc().snapshot(&events, now);
        assert_eq!(snap.used, 15_000);
        assert_eq!(snap.samples.len(), 2);
        assert!(!snap.is_limited);
        assert_eq!(snap.severity, RateLimitSeverity::Normal);
    }

    #[test]
    fn severity_thresholds() {
        let now = at_moscow(10, 0);
        let warn = evc().snapshot(&[ev(1, now - 60_000, 42_000)], now);
        assert_eq!(warn.severity, RateLimitSeverity::Warning);
        let crit = evc().snapshot(&[ev(1, now - 60_000, 54_000)], now);
        assert_eq!(crit.severity, RateLimitSeverity::Critical);
    }

    #[test]
    fn local_hour_helpers() {
        let p = evc_preset();
        let off = p.timezone_offset_ms();
        assert_eq!(local_hour(at_moscow(7, 59), off), 7);
        assert_eq!(local_hour(at_moscow(8, 0), off), 8);
        assert_eq!(local_hour(at_moscow(20, 59), off), 20);
        assert_eq!(local_hour(at_moscow(21, 0), off), 21);
        let (from, to) = p.work_hours().expect("scheduled preset");
        assert!(working_hours(at_moscow(8, 0), off, from, to));
        assert!(!working_hours(at_moscow(21, 0), off, from, to));
    }

    #[test]
    fn null_work_hours_enforced_around_the_clock() {
        let mut preset = evc_preset();
        preset.work_from_hour = None;
        preset.work_to_hour = None;
        let policy = EvcSlidingWindow { preset };
        let now = at_moscow(3, 0);
        let snap = policy.snapshot(&[ev(1, now - 60_000, 70_000)], now);
        assert!(snap.is_enforced);
        assert!(snap.is_limited);
        assert_eq!(snap.severity, RateLimitSeverity::Limited);
        assert!(snap.next_enforce_at.is_none());
    }

    #[test]
    fn preset_deserializes_null_work_hours() {
        let preset: RateLimitPreset = serde_json::from_str(
            r#"{
                "providerId": "alfagen",
                "policyId": "evc-sliding-window",
                "label": "EVC",
                "limit": 60000,
                "windowMinutes": 30,
                "workFromHour": null,
                "workToHour": null,
                "timezoneOffsetHours": 3
            }"#,
        )
        .unwrap();
        assert!(preset.work_hours().is_none());
    }
}
