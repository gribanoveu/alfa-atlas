//! Pluggable LLM rate-limit policies.
//!
//! The algorithm lives behind [`RateLimitPolicy`]; the UI only sees a
//! stable [`RateLimitSnapshot`]. Swapping the formula (sliding window →
//! token bucket, different hours, different caps) means a new
//! implementation + a one-line change in [`policy_for`] — not a UI rewrite.
//!
//! Policies are pure: they do not own the event log and do no I/O.
//! Persistence and recording live in `services::llm_rate_limit`.
//!
//! The server counts three things in the same window — prompt tokens,
//! completion tokens and request count — and refuses when *any* of them
//! reaches its cap. A snapshot therefore carries one
//! [`RateLimitResource`] per counter plus an aggregate (worst-off
//! resource) for the status-bar chip, which has room for a single number.

use serde::{Deserialize, Serialize};

/// One recorded LLM round: the tokens it cost and, implicitly, one request.
/// `at_ms` is Unix epoch milliseconds (UTC). Policies decide which events
/// fall inside their window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageEvent {
    pub id: u64,
    pub at_ms: i64,
    /// Completion tokens. Named `tokens` because that is what the field was
    /// called when it was the only counter — renaming it would orphan every
    /// `llm-rate-limit.json` already on disk.
    pub tokens: u32,
    /// Prompt tokens. `default` for the same reason: files written before
    /// prompt tracking existed have no such key, and they must still load.
    #[serde(default)]
    pub prompt_tokens: u32,
}

/// Which counter a [`RateLimitResource`] describes. The UI switches on this
/// rather than on a label, so wording stays a frontend concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RateLimitResourceKind {
    Prompt,
    Completion,
    Requests,
}

/// Visual severity for the status-bar chip. Computed by the policy so
/// thresholds (70% / 90%) stay swappable with the algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RateLimitSeverity {
    // Ordered least → most alarming: `Ord` picks the worst resource.
    OffHours,
    Normal,
    Warning,
    Critical,
    Limited,
}

/// One upcoming release of tokens from the sliding window (or equivalent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitRelease {
    /// Epoch ms when this round's spend leaves the window.
    pub at: i64,
    pub tokens: u32,
    #[serde(default)]
    pub prompt_tokens: u32,
}

/// One sample still inside the active window — drives the timeline tab.
/// Policies without a window return an empty list; the UI then hides that tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSample {
    pub id: String,
    pub at: i64,
    pub tokens: u32,
    #[serde(default)]
    pub prompt_tokens: u32,
    pub expires_at: i64,
}

/// One of the three counters the server enforces, as its own little
/// snapshot. Everything the UI needs to draw a labelled bar and say when
/// this particular counter frees up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitResource {
    pub kind: RateLimitResourceKind,
    pub used: u32,
    pub remaining: u32,
    pub limit: u32,
    pub is_limited: bool,
    pub severity: RateLimitSeverity,
    /// When this counter drops back below its cap. `None` unless limited.
    pub retry_until: Option<i64>,
    /// When the oldest round in the window expires, and how much of *this*
    /// counter it gives back.
    pub next_release_at: Option<i64>,
    pub next_release_amount: u32,
}

/// Stable IPC DTO. The frontend must not hard-code window length, limits, or
/// working hours — everything it needs is here.
///
/// `used`/`remaining`/`limit`/`severity`/`retry_until` describe the
/// *driving* resource (the one closest to its cap); `resources` carries all
/// three. The chip renders the aggregate, the popover the breakdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSnapshot {
    pub policy_id: String,
    pub label: String,
    pub used: u32,
    pub remaining: u32,
    pub limit: u32,
    /// Which counter the aggregate numbers above describe. `None` only for
    /// policies with no resources at all (noop).
    pub driving_kind: Option<RateLimitResourceKind>,
    pub resources: Vec<RateLimitResource>,
    /// `None` for policies that have no sliding window.
    pub window_ms: Option<i64>,
    pub is_enforced: bool,
    pub is_limited: bool,
    pub severity: RateLimitSeverity,
    pub retry_until: Option<i64>,
    pub next_release_at: Option<i64>,
    pub next_enforce_at: Option<i64>,
    /// True when the schedule says "off hours" but the user asked to keep
    /// counting anyway — the UI explains why the chip is live at 3am.
    pub off_hours_override: bool,
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
    /// Completion-token cap. Keeps its bare name for backwards
    /// compatibility with manifests written before the other two existed.
    pub limit: u32,
    /// Prompt-token cap. `0` disables that counter.
    #[serde(default)]
    pub prompt_limit: u32,
    /// Request-count cap. `0` disables that counter.
    #[serde(default)]
    pub request_limit: u32,
    pub window_minutes: u32,
    /// Inclusive local start hour. JSON `null` (or omit) together with
    /// [`Self::work_to_hour`] means the sliding window is enforced 24/7.
    #[serde(default)]
    pub work_from_hour: Option<u8>,
    /// Exclusive local end hour. Both hours must be set to gate by schedule;
    /// either `null` → always on.
    #[serde(default)]
    pub work_to_hour: Option<u8>,
    /// When true, Saturday and Sunday are off hours whatever the clock says.
    /// Only meaningful together with the two hours above.
    #[serde(default)]
    pub work_weekdays_only: bool,
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
///
/// `enforce_off_hours` comes from user settings, not from the manifest: the
/// schedule describes the *server*, and this is the user's choice to keep
/// counting outside it (see `LlmSettings::rate_limit_off_hours_enforced`).
pub fn policy_for(
    preset: Option<&RateLimitPreset>,
    enforce_off_hours: bool,
) -> Box<dyn RateLimitPolicy> {
    match preset {
        Some(p) if p.policy_id == "evc-sliding-window" => Box::new(EvcSlidingWindow {
            preset: p.clone(),
            enforce_off_hours,
        }),
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
            driving_kind: None,
            resources: Vec::new(),
            window_ms: None,
            is_enforced: false,
            is_limited: false,
            severity: RateLimitSeverity::OffHours,
            retry_until: None,
            next_release_at: None,
            next_enforce_at: None,
            off_hours_override: false,
            releases: Vec::new(),
            samples: Vec::new(),
        }
    }
}

/// Sliding window over prompt tokens, completion tokens and request count,
/// with an optional working-hours (and working-days) gate.
/// Numbers come from [`RateLimitPreset`], not from this struct.
pub struct EvcSlidingWindow {
    preset: RateLimitPreset,
    /// Count during off hours anyway. User setting; see [`policy_for`].
    enforce_off_hours: bool,
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

        let on_schedule = match self.preset.work_hours() {
            Some((from, to)) => {
                working_hours(now_ms, offset_ms, from, to, self.preset.work_weekdays_only)
            }
            None => true,
        };
        let is_enforced = on_schedule || self.enforce_off_hours;
        let off_hours_override = !on_schedule && self.enforce_off_hours;

        let resources: Vec<RateLimitResource> = [
            (
                RateLimitResourceKind::Prompt,
                self.preset.prompt_limit,
                (|e: &UsageEvent| e.prompt_tokens) as fn(&UsageEvent) -> u32,
            ),
            (
                RateLimitResourceKind::Completion,
                self.preset.limit,
                (|e: &UsageEvent| e.tokens) as fn(&UsageEvent) -> u32,
            ),
            (
                RateLimitResourceKind::Requests,
                self.preset.request_limit,
                (|_e: &UsageEvent| 1) as fn(&UsageEvent) -> u32,
            ),
        ]
        .into_iter()
        .filter(|(_, limit, _)| *limit > 0)
        .map(|(kind, limit, amount)| {
            self.resource(kind, limit, &active, window_ms, is_enforced, amount)
        })
        .collect();

        let releases: Vec<RateLimitRelease> = active
            .iter()
            .map(|e| RateLimitRelease {
                at: e.at_ms + window_ms,
                tokens: e.tokens,
                prompt_tokens: e.prompt_tokens,
            })
            .collect();

        let samples: Vec<RateLimitSample> = active
            .iter()
            .map(|e| RateLimitSample {
                id: e.id.to_string(),
                at: e.at_ms,
                tokens: e.tokens,
                prompt_tokens: e.prompt_tokens,
                expires_at: e.at_ms + window_ms,
            })
            .collect();

        // The chip has room for one number, so it shows whichever counter is
        // closest to its cap — the one that will actually stop the next
        // request.
        let driving = resources
            .iter()
            .max_by(|a, b| ratio(a).total_cmp(&ratio(b)))
            .cloned();

        let is_limited = resources.iter().any(|r| r.is_limited);
        let severity = if !is_enforced {
            RateLimitSeverity::OffHours
        } else {
            resources
                .iter()
                .map(|r| r.severity)
                .max()
                .unwrap_or(RateLimitSeverity::Normal)
        };
        // Every limited counter has to clear, so the latest of them wins.
        let retry_until = resources.iter().filter_map(|r| r.retry_until).max();

        // Only worth showing while the chip is actually idle: with the
        // off-hours override on, the window is already being counted and
        // "limits resume at 9:00" would be a lie.
        let next_enforce_at = match self.preset.work_hours() {
            Some((from, _)) if !is_enforced => Some(next_work_start(
                now_ms,
                offset_ms,
                from,
                self.preset.work_weekdays_only,
            )),
            _ => None,
        };

        RateLimitSnapshot {
            policy_id: self.id().to_string(),
            label: self.label().to_string(),
            used: driving.as_ref().map(|r| r.used).unwrap_or(0),
            remaining: driving.as_ref().map(|r| r.remaining).unwrap_or(0),
            limit: driving.as_ref().map(|r| r.limit).unwrap_or(0),
            driving_kind: driving.as_ref().map(|r| r.kind),
            resources,
            window_ms: Some(window_ms),
            is_enforced,
            is_limited,
            severity,
            retry_until,
            next_release_at: releases.first().map(|r| r.at),
            next_enforce_at,
            off_hours_override,
            releases,
            samples,
        }
    }
}

/// Share of a counter's cap that is already spent. Feeds "which resource
/// drives the chip"; a limited counter can exceed 1.0.
fn ratio(r: &RateLimitResource) -> f64 {
    if r.limit == 0 {
        0.0
    } else {
        r.used as f64 / r.limit as f64
    }
}

impl EvcSlidingWindow {
    /// One counter's slice of the window. `amount` extracts this counter's
    /// contribution from an event — tokens for the two token counters, a
    /// flat 1 for the request count.
    fn resource(
        &self,
        kind: RateLimitResourceKind,
        limit: u32,
        active: &[&UsageEvent],
        window_ms: i64,
        is_enforced: bool,
        amount: fn(&UsageEvent) -> u32,
    ) -> RateLimitResource {
        let used: u32 = active.iter().map(|e| amount(e)).sum();
        let is_limited = is_enforced && used >= limit;
        let retry_until = if is_limited {
            compute_retry_until(active, window_ms, used, limit, amount)
        } else {
            None
        };
        let severity = if !is_enforced {
            RateLimitSeverity::OffHours
        } else if is_limited {
            RateLimitSeverity::Limited
        } else {
            let pct = used as f64 / limit as f64;
            if pct >= self.preset.crit_ratio {
                RateLimitSeverity::Critical
            } else if pct >= self.preset.warn_ratio {
                RateLimitSeverity::Warning
            } else {
                RateLimitSeverity::Normal
            }
        };
        RateLimitResource {
            kind,
            used,
            remaining: limit.saturating_sub(used),
            limit,
            is_limited,
            severity,
            retry_until,
            next_release_at: active.first().map(|e| e.at_ms + window_ms),
            next_release_amount: active.first().map(|e| amount(e)).unwrap_or(0),
        }
    }
}

/// Earliest moment at which dropping oldest events brings `used` below `limit`.
///
/// The server answers "when the oldest record leaves the window", which is
/// the same thing whenever that one record is enough — and optimistic when
/// it is not (its own docs admit the retry may be refused again). We report
/// the moment a request will genuinely pass instead of the moment it is
/// worth trying.
fn compute_retry_until(
    active: &[&UsageEvent],
    window_ms: i64,
    used: u32,
    limit: u32,
    amount: fn(&UsageEvent) -> u32,
) -> Option<i64> {
    let mut acc = used;
    for e in active {
        acc = acc.saturating_sub(amount(e));
        if acc < limit {
            return Some(e.at_ms + window_ms);
        }
    }
    active.last().map(|e| e.at_ms + window_ms)
}

fn working_hours(
    now_ms: i64,
    offset_ms: i64,
    from_hour: u8,
    to_hour: u8,
    weekdays_only: bool,
) -> bool {
    if weekdays_only && is_weekend(now_ms, offset_ms) {
        return false;
    }
    let hour = local_hour(now_ms, offset_ms);
    hour >= from_hour as i64 && hour < to_hour as i64
}

fn local_hour(now_ms: i64, offset_ms: i64) -> i64 {
    let local_ms = now_ms + offset_ms;
    let day_ms = 24 * 60 * 60 * 1000;
    let ms_in_day = ((local_ms % day_ms) + day_ms) % day_ms;
    ms_in_day / (60 * 60 * 1000)
}

/// Days since the epoch, in the preset's timezone. Floor division so dates
/// before 1970 (and negative offsets near it) don't round towards zero.
fn local_days(now_ms: i64, offset_ms: i64) -> i64 {
    let day_ms = 24 * 60 * 60 * 1000;
    (now_ms + offset_ms).div_euclid(day_ms)
}

/// 0 = Sunday … 6 = Saturday. 1970-01-01 was a Thursday, hence the `+ 4`.
fn local_weekday(now_ms: i64, offset_ms: i64) -> i64 {
    (local_days(now_ms, offset_ms) + 4).rem_euclid(7)
}

fn is_weekend(now_ms: i64, offset_ms: i64) -> bool {
    matches!(local_weekday(now_ms, offset_ms), 0 | 6)
}

/// Next `from_hour:00` in the preset timezone strictly after `now_ms` (or
/// today if still before that hour), skipping weekends when the schedule is
/// weekdays-only.
fn next_work_start(now_ms: i64, offset_ms: i64, from_hour: u8, weekdays_only: bool) -> i64 {
    let day_ms = 24 * 60 * 60 * 1000;
    let local_ms = now_ms + offset_ms;
    let ms_in_day = ((local_ms % day_ms) + day_ms) % day_ms;
    let day_start_local = local_ms - ms_in_day;
    let today_start_local = day_start_local + from_hour as i64 * 60 * 60 * 1000;
    let mut candidate = today_start_local - offset_ms;
    if now_ms >= candidate {
        candidate += day_ms;
    }
    if weekdays_only {
        // At most two hops (Sat → Mon), but loop rather than special-case.
        while is_weekend(candidate, offset_ms) {
            candidate += day_ms;
        }
    }
    candidate
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
            prompt_limit: 10_000_000,
            request_limit: 1_000,
            window_minutes: 20,
            work_from_hour: Some(9),
            work_to_hour: Some(19),
            work_weekdays_only: true,
            timezone_offset_hours: 3,
            warn_ratio: 0.70,
            crit_ratio: 0.90,
        }
    }

    fn evc() -> EvcSlidingWindow {
        EvcSlidingWindow {
            preset: evc_preset(),
            enforce_off_hours: false,
        }
    }

    /// Build a UTC epoch ms that is `h:m` in Moscow on 2026-07-15, a
    /// **Wednesday** (Moscow = UTC+3, no DST).
    fn at_moscow(h: i64, m: i64) -> i64 {
        const JUL_15_2026_UTC_MIDNIGHT: i64 = 1_784_073_600_000;
        let msk_midnight = JUL_15_2026_UTC_MIDNIGHT - evc_preset().timezone_offset_ms();
        msk_midnight + h * 3_600_000 + m * 60_000
    }

    const DAY_MS: i64 = 24 * 3_600_000;

    fn ev(id: u64, at_ms: i64, tokens: u32) -> UsageEvent {
        UsageEvent {
            id,
            at_ms,
            tokens,
            prompt_tokens: 0,
        }
    }

    fn ev_full(id: u64, at_ms: i64, prompt: u32, completion: u32) -> UsageEvent {
        UsageEvent {
            id,
            at_ms,
            tokens: completion,
            prompt_tokens: prompt,
        }
    }

    fn resource(snap: &RateLimitSnapshot, kind: RateLimitResourceKind) -> &RateLimitResource {
        snap.resources
            .iter()
            .find(|r| r.kind == kind)
            .expect("resource present")
    }

    #[test]
    fn policy_for_evc_preset_vs_none() {
        assert_eq!(
            policy_for(Some(&evc_preset()), false).id(),
            "evc-sliding-window"
        );
        assert_eq!(policy_for(None, false).id(), "none");
    }

    #[test]
    fn completion_limit_retry_when_the_oldest_round_expires() {
        // 65k completion tokens spent inside the 20-minute window; dropping
        // the oldest 20k brings it under 60k, so that record's expiry is the
        // answer.
        let events = vec![
            ev(1, at_moscow(11, 51), 20_000),
            ev(2, at_moscow(11, 55), 15_000),
            ev(3, at_moscow(11, 57), 15_000),
            ev(4, at_moscow(11, 58), 15_000),
        ];
        let now = at_moscow(12, 0);
        let snap = evc().snapshot(&events, now);
        assert!(snap.is_enforced);
        assert!(snap.is_limited);
        assert_eq!(snap.used, 65_000);
        assert_eq!(snap.driving_kind, Some(RateLimitResourceKind::Completion));
        assert_eq!(snap.severity, RateLimitSeverity::Limited);
        assert_eq!(snap.retry_until, Some(at_moscow(12, 11)));
        assert_eq!(snap.retry_until.unwrap() - now, 11 * 60_000);
    }

    #[test]
    fn all_three_counters_are_tracked() {
        let now = at_moscow(12, 0);
        let events = vec![
            ev_full(1, now - 60_000, 400_000, 5_000),
            ev_full(2, now - 30_000, 600_000, 7_000),
        ];
        let snap = evc().snapshot(&events, now);
        assert_eq!(resource(&snap, RateLimitResourceKind::Prompt).used, 1_000_000);
        assert_eq!(resource(&snap, RateLimitResourceKind::Completion).used, 12_000);
        assert_eq!(resource(&snap, RateLimitResourceKind::Requests).used, 2);
        assert_eq!(resource(&snap, RateLimitResourceKind::Requests).limit, 1_000);
        assert!(!snap.is_limited);
    }

    #[test]
    fn the_request_count_can_limit_on_its_own() {
        // Tiny replies, but a thousand tool rounds: completion tokens are
        // nowhere near their cap and the request counter still says stop.
        let now = at_moscow(12, 0);
        let events: Vec<UsageEvent> = (0..1_000)
            .map(|i| ev(i, now - 600_000 + i as i64, 10))
            .collect();
        let snap = evc().snapshot(&events, now);
        assert!(snap.is_limited);
        assert_eq!(snap.driving_kind, Some(RateLimitResourceKind::Requests));
        assert_eq!(snap.severity, RateLimitSeverity::Limited);
        assert!(!resource(&snap, RateLimitResourceKind::Completion).is_limited);
        // One round has to age out before the 1001st is allowed.
        assert_eq!(
            snap.retry_until,
            Some(now - 600_000 + evc_preset().window_ms())
        );
    }

    #[test]
    fn retry_waits_for_the_slowest_of_several_limited_counters() {
        let now = at_moscow(12, 0);
        // 999 cheap rounds at the head of the window: one of them ageing
        // out is enough for the request counter…
        let mut events: Vec<UsageEvent> = (0..999)
            .map(|i| ev(i, now - 900_000 + i as i64, 10))
            .collect();
        // …but the completion counter only clears when this late, expensive
        // round leaves, which happens much later.
        events.push(ev_full(999, now - 60_000, 0, 60_000));
        let snap = evc().snapshot(&events, now);
        let completion = resource(&snap, RateLimitResourceKind::Completion);
        let requests = resource(&snap, RateLimitResourceKind::Requests);
        assert!(completion.is_limited);
        assert!(requests.is_limited);
        assert_eq!(snap.retry_until, completion.retry_until);
        assert!(completion.retry_until > requests.retry_until);
        assert_eq!(
            completion.retry_until,
            Some(now - 60_000 + evc_preset().window_ms())
        );
    }

    #[test]
    fn a_disabled_counter_is_absent_from_the_snapshot() {
        let mut preset = evc_preset();
        preset.prompt_limit = 0;
        let policy = EvcSlidingWindow {
            preset,
            enforce_off_hours: false,
        };
        let now = at_moscow(12, 0);
        let snap = policy.snapshot(&[ev_full(1, now - 60_000, 9_000_000, 10)], now);
        assert!(snap
            .resources
            .iter()
            .all(|r| r.kind != RateLimitResourceKind::Prompt));
        assert!(!snap.is_limited);
    }

    #[test]
    fn off_hours_before_nine_not_enforced() {
        let events = vec![ev(1, at_moscow(8, 50), 70_000)];
        let now = at_moscow(8, 55);
        let snap = evc().snapshot(&events, now);
        assert!(!snap.is_enforced);
        assert!(!snap.is_limited);
        assert_eq!(snap.severity, RateLimitSeverity::OffHours);
        assert_eq!(resource(&snap, RateLimitResourceKind::Completion).used, 70_000);
        assert_eq!(snap.next_enforce_at, Some(at_moscow(9, 0)));
        assert!(!snap.off_hours_override);
    }

    #[test]
    fn off_hours_after_nineteen_not_enforced() {
        let now = at_moscow(19, 5);
        let snap = evc().snapshot(&[ev(1, at_moscow(19, 0), 70_000)], now);
        assert!(!snap.is_enforced);
        // Wednesday evening → next morning, no weekend hop.
        assert_eq!(snap.next_enforce_at, Some(at_moscow(9, 0) + DAY_MS));
    }

    #[test]
    fn weekends_are_off_hours_even_at_noon() {
        // 2026-07-18 is the Saturday of that week.
        let saturday_noon = at_moscow(12, 0) + 3 * DAY_MS;
        let snap = evc().snapshot(&[ev(1, saturday_noon - 60_000, 70_000)], saturday_noon);
        assert!(!snap.is_enforced);
        assert_eq!(snap.severity, RateLimitSeverity::OffHours);
        // …and the next enforced moment is Monday morning, not Sunday.
        assert_eq!(snap.next_enforce_at, Some(at_moscow(9, 0) + 5 * DAY_MS));
    }

    #[test]
    fn friday_evening_points_at_monday() {
        let friday_evening = at_moscow(20, 0) + 2 * DAY_MS;
        let snap = evc().snapshot(&[], friday_evening);
        assert!(!snap.is_enforced);
        assert_eq!(snap.next_enforce_at, Some(at_moscow(9, 0) + 5 * DAY_MS));
    }

    #[test]
    fn off_hours_override_keeps_counting() {
        let policy = EvcSlidingWindow {
            preset: evc_preset(),
            enforce_off_hours: true,
        };
        let saturday_noon = at_moscow(12, 0) + 3 * DAY_MS;
        let snap = policy.snapshot(&[ev(1, saturday_noon - 60_000, 70_000)], saturday_noon);
        assert!(snap.is_enforced);
        assert!(snap.is_limited);
        assert!(snap.off_hours_override);
        assert_eq!(snap.severity, RateLimitSeverity::Limited);
        // Nothing to wait for on the clock — the window is what frees up.
        assert!(snap.next_enforce_at.is_none());
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
        assert_eq!(resource(&snap, RateLimitResourceKind::Completion).used, 15_000);
        assert_eq!(resource(&snap, RateLimitResourceKind::Requests).used, 2);
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
    fn the_worst_resource_wins_the_aggregate() {
        let now = at_moscow(10, 0);
        // Completion is quiet, prompt is critical: the chip must show prompt.
        let snap = evc().snapshot(&[ev_full(1, now - 60_000, 9_500_000, 1_000)], now);
        assert_eq!(snap.driving_kind, Some(RateLimitResourceKind::Prompt));
        assert_eq!(snap.used, 9_500_000);
        assert_eq!(snap.limit, 10_000_000);
        assert_eq!(snap.severity, RateLimitSeverity::Critical);
    }

    #[test]
    fn local_hour_and_weekday_helpers() {
        let p = evc_preset();
        let off = p.timezone_offset_ms();
        assert_eq!(local_hour(at_moscow(8, 59), off), 8);
        assert_eq!(local_hour(at_moscow(9, 0), off), 9);
        assert_eq!(local_hour(at_moscow(18, 59), off), 18);
        assert_eq!(local_hour(at_moscow(19, 0), off), 19);
        // 2026-07-15 is a Wednesday (3), so +3 days is Saturday (6).
        assert_eq!(local_weekday(at_moscow(12, 0), off), 3);
        assert_eq!(local_weekday(at_moscow(12, 0) + 3 * DAY_MS, off), 6);
        assert!(is_weekend(at_moscow(12, 0) + 3 * DAY_MS, off));
        assert!(is_weekend(at_moscow(12, 0) + 4 * DAY_MS, off));
        assert!(!is_weekend(at_moscow(12, 0) + 5 * DAY_MS, off));
        let (from, to) = p.work_hours().expect("scheduled preset");
        assert!(working_hours(at_moscow(9, 0), off, from, to, true));
        assert!(!working_hours(at_moscow(19, 0), off, from, to, true));
        // Same clock time, weekend: only the weekdays-only rule rejects it.
        let sat_noon = at_moscow(12, 0) + 3 * DAY_MS;
        assert!(working_hours(sat_noon, off, from, to, false));
        assert!(!working_hours(sat_noon, off, from, to, true));
    }

    #[test]
    fn null_work_hours_enforced_around_the_clock() {
        let mut preset = evc_preset();
        preset.work_from_hour = None;
        preset.work_to_hour = None;
        let policy = EvcSlidingWindow {
            preset,
            enforce_off_hours: false,
        };
        let now = at_moscow(3, 0);
        let snap = policy.snapshot(&[ev(1, now - 60_000, 70_000)], now);
        assert!(snap.is_enforced);
        assert!(snap.is_limited);
        assert_eq!(snap.severity, RateLimitSeverity::Limited);
        assert!(snap.next_enforce_at.is_none());
    }

    #[test]
    fn preset_deserializes_defaults_for_new_fields() {
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
        // A manifest from before the other two counters existed keeps
        // behaving as a completion-only rule.
        assert_eq!(preset.prompt_limit, 0);
        assert_eq!(preset.request_limit, 0);
        assert!(!preset.work_weekdays_only);
    }

    #[test]
    fn usage_event_loads_without_prompt_tokens() {
        let event: UsageEvent =
            serde_json::from_str(r#"{"id":1,"atMs":1700000000000,"tokens":500}"#).unwrap();
        assert_eq!(event.prompt_tokens, 0);
    }
}
