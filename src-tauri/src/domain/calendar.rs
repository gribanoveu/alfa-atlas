//! OWA/Exchange calendar: connection settings and the pure shape + rules of a
//! meeting. No HTTP, no NTLM, no `ureq` types leak in here — the client lives
//! in `infra::owa_client`, the auth in `infra::owa_auth`.
//!
//! Everything here is UTC. The spike proved `TimeZoneContext.Id = "UTC"` is
//! accepted and the server returns unambiguous UTC instants, so the backend
//! never touches timezones — the user's display zone is a frontend `Intl`
//! concern (`CalendarSettings::display_time_zone`).
//!
//! Two layers: a build-time `CalendarPreset` from the `calendar`
//! section of `assets/llm/system_providers.yaml` (ships the instance URL and
//! the corporate CA), and the user's `CalendarSettings` on top. The domain
//! password is never in `CalendarSettings` — it lives encrypted in
//! `infra::owa_credentials_store` (`SecretPurpose::CalendarPassword`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Build-time defaults, from the manifest's `calendar` section. All optional:
/// no `calendar` section is valid and means the user configures everything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CalendarPreset {
    pub base_url: Option<String>,
    pub trusted_cert_pem: Option<String>,
}

/// Minutes before a meeting that the reminder chimes, when the user has never
/// touched the setting. `0` disables reminders.
pub const DEFAULT_REMINDER_MINUTES: u32 = 5;
/// Nobody wants a reminder two hours out; also bounds what the UI can store.
pub const MAX_REMINDER_MINUTES: u32 = 120;

/// The user layer. Empty `base_url` falls back to `CalendarPreset`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CalendarSettings {
    /// Exchange root, e.g. `https://owa.company.ru` — no `/owa/...` path.
    pub base_url: String,
    /// `DOMAIN\user`, e.g. `MOSCOW\ivanov`.
    pub username: String,
    /// IANA zone for *display only* (e.g. `Europe/Moscow`). Empty = the
    /// viewer's OS zone. The backend stays UTC regardless of this.
    pub display_time_zone: String,
    /// Keep the domain password on disk (encrypted) between launches.
    /// Default `false`: the password is an account credential, so
    /// remembering it is an explicit opt-in, not the default.
    pub remember_password: bool,
    /// PEM bundle replacing the public trust roots. `None` falls back to the
    /// build's certificate, if it ships one.
    pub trusted_cert_pem: Option<String>,
    /// Chime this many minutes before a meeting starts; `0` is off. The
    /// countdown itself runs on the frontend (see `useCalendarReminders`).
    pub reminder_minutes: u32,
}

// Hand-written so a settings file predating `reminder_minutes` — and a fresh
// install — get reminders on at the default lead, not silently at zero.
impl Default for CalendarSettings {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            username: String::new(),
            display_time_zone: String::new(),
            remember_password: false,
            trusted_cert_pem: None,
            reminder_minutes: DEFAULT_REMINDER_MINUTES,
        }
    }
}

impl CalendarSettings {
    /// Whether there is any host to talk to at all (own value or, once
    /// resolved against the preset, the build default). Checked post-resolve.
    pub fn is_addressable(&self) -> bool {
        !self.base_url.trim().is_empty()
    }
}

/// What the settings tab reads: the user's own values plus what the build
/// would fall back to for each field left empty.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSettingsView {
    pub settings: CalendarSettings,
    pub bundled_base_url: Option<String>,
    pub has_bundled_cert: bool,
    /// Whether a password is currently remembered on disk — the form shows
    /// "saved" vs "not saved" without ever reading the password back.
    pub has_password: bool,
}

/// Snapshot the panel renders without a network call.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CalendarStatus {
    /// A host is configured (own or preset) and a username is set.
    pub configured: bool,
    /// A live session exists (a password is available this run).
    pub connected: bool,
    /// Breaker tripped after repeated auth failures — no auto retry.
    pub circuit_open: bool,
    pub cached_count: usize,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MeetingResponseType {
    Accepted,
    Tentative,
    Declined,
    Organizer,
    NotResponded,
}

impl MeetingResponseType {
    /// Maps the OWA `ResponseType` string. Unknown/empty -> `NotResponded`.
    pub fn from_owa(s: Option<&str>) -> Self {
        match s {
            Some("Accept") => Self::Accepted,
            Some("Tentative") => Self::Tentative,
            Some("Decline") => Self::Declined,
            Some("Organizer") => Self::Organizer,
            _ => Self::NotResponded,
        }
    }
}

/// An RSVP action the viewer can take on an invitation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RsvpAction {
    Accept,
    Tentative,
    Decline,
}

impl RsvpAction {
    /// The EWS `CreateItem` element name for this action.
    pub fn ews_element(self) -> &'static str {
        match self {
            RsvpAction::Accept => "AcceptItem",
            RsvpAction::Tentative => "TentativelyAcceptItem",
            RsvpAction::Decline => "DeclineItem",
        }
    }

    /// The response type this action results in — used to reflect the new
    /// state in the UI without a re-sync.
    pub fn resulting_response(self) -> MeetingResponseType {
        match self {
            RsvpAction::Accept => MeetingResponseType::Accepted,
            RsvpAction::Tentative => MeetingResponseType::Tentative,
            RsvpAction::Decline => MeetingResponseType::Declined,
        }
    }
}

/// One meeting participant, from the on-demand details request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventAttendee {
    pub name: String,
    pub email: Option<String>,
    /// Required vs optional invitee.
    pub required: bool,
    pub response: MeetingResponseType,
}

/// The extra content fetched only when a meeting is opened (GetCalendarEvent):
/// the attendee lists and a plain-text body preview. HTML is deliberately
/// reduced to text — the panel never renders server HTML.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventDetails {
    pub attendees: Vec<EventAttendee>,
    pub body_text: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MeetingPlatform {
    Teams,
    Zoom,
    GoogleMeet,
    Webex,
    Telemost,
    Jazz,
    Vk,
    Ktalk,
    MtsLink,
    Generic,
}

/// A calendar meeting, already mapped to UTC and safe to hand the frontend.
/// Read-only v1: no attendees / body / change_key — those arrive with the
/// details (Phase 2) and RSVP (Phase 3) work.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    /// Revision key, needed to fetch details and (later) to RSVP. May be
    /// absent on some items.
    pub change_key: Option<String>,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub is_all_day: bool,
    pub is_cancelled: bool,
    pub is_organizer: bool,
    pub organizer: Option<String>,
    pub location: Option<String>,
    /// Already validated + normalized to `https://` by `safe_url`, or `None`.
    pub join_url: Option<String>,
    pub platform: MeetingPlatform,
    pub response_type: MeetingResponseType,
}

impl CalendarEvent {
    /// A meeting counts as cancelled when the server flags it *or* the subject
    /// carries a cancellation prefix (common when a cancellation arrives via
    /// an external mail client). The frontend mirrors this in
    /// `isEffectivelyCancelled` for rendering; here it gates the reminder.
    pub fn is_effectively_cancelled(&self) -> bool {
        if self.is_cancelled {
            return true;
        }
        let t = self.title.trim().to_lowercase();
        t.starts_with("отменено:") || t.starts_with("cancelled:") || t.starts_with("canceled:")
    }
}

/// Whether this meeting's reminder is due at `now`, and how many minutes out
/// it is (rounded, never below 1 — "через 0 мин" reads as a bug).
///
/// `lead_minutes` of 0 disables reminders. A meeting already under way is
/// silent: if the lead window passed while the app was closed, saying "через
/// 5 минут" about something that started ten minutes ago is worse than
/// nothing. All-day blocks, cancelled meetings and ones the user declined
/// never chime.
pub fn reminder_due_in(
    event: &CalendarEvent,
    lead_minutes: u32,
    now: DateTime<Utc>,
) -> Option<u32> {
    if lead_minutes == 0
        || event.is_all_day
        || event.response_type == MeetingResponseType::Declined
        || event.is_effectively_cancelled()
    {
        return None;
    }
    let secs = (event.start - now).num_seconds();
    (secs > 0 && secs <= i64::from(lead_minutes) * 60)
        .then(|| ((secs as f64) / 60.0).round().max(1.0) as u32)
}

/// What the background calendar loops report outward. A port, like
/// `domain::workspace_index::WorkspaceIndexEventSink`: the services never
/// learn what is on the other side, and `commands::calendar_events` is the
/// only place these become Tauri events.
#[derive(Debug, Clone)]
pub enum CalendarNotice {
    /// A sync refreshed the cache — the whole window, as the panel wants it.
    Updated(Vec<CalendarEvent>),
    /// A meeting starts in `minutes`. Emitted once per meeting per run.
    Reminder { event: CalendarEvent, minutes: u32 },
}

pub type CalendarNoticeSink = std::sync::Arc<dyn Fn(CalendarNotice) + Send + Sync>;

/// Validates a user-entered server URL before it is saved. Empty is allowed
/// (the build preset fills in). Otherwise it must be `https://` with a real
/// hostname — not `http`, not an IP literal — so a domain password is only
/// ever sent over TLS to a named host the admin configured, never to a bare
/// address that could be anything.
pub fn validate_base_url(base_url: &str) -> Result<(), String> {
    let t = base_url.trim();
    if t.is_empty() {
        return Ok(());
    }
    if !t.to_lowercase().starts_with("https://") {
        return Err("Адрес должен начинаться с https://".to_string());
    }
    let url = url::Url::parse(t).map_err(|_| "Некорректный адрес сервера".to_string())?;
    match url.host() {
        Some(url::Host::Domain(h)) if !h.is_empty() => Ok(()),
        Some(_) => Err("Укажите доменное имя сервера, а не IP-адрес".to_string()),
        None => Err("В адресе не указан хост".to_string()),
    }
}

/// The AD-lockout guard as a pure state machine: after `MAX_AUTH_FAILURES`
/// consecutive auth failures it opens and nothing retries automatically until
/// `reset` (the user re-entering the password). Separated from the session so
/// the transitions are unit-tested without touching the network.
#[derive(Debug, Default)]
pub struct CircuitBreaker {
    failures: u8,
    open: bool,
}

impl CircuitBreaker {
    /// Two consecutive auth failures trip it — a hard ceiling, never approach
    /// the AD lockout threshold.
    pub const MAX_AUTH_FAILURES: u8 = 2;

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// A successful request clears the streak.
    pub fn record_success(&mut self) {
        self.failures = 0;
        self.open = false;
    }

    /// Records an auth failure; returns `true` if this tripped the breaker.
    pub fn record_auth_failure(&mut self) -> bool {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= Self::MAX_AUTH_FAILURES {
            self.open = true;
        }
        self.open
    }

    /// Manual "retry login" — clears the breaker so a sync may run again.
    pub fn reset(&mut self) {
        self.failures = 0;
        self.open = false;
    }
}

#[derive(Debug, Error)]
pub enum CalendarError {
    #[error("calendar is not configured")]
    NotConfigured,
    #[error("no password stored — enter it to connect")]
    MissingPassword,
    #[error("authentication failed")]
    AuthFailed,
    /// The circuit breaker is open after repeated auth failures; refuse to try
    /// again automatically so we never lock the AD account.
    #[error("too many failed logins — retry manually")]
    CircuitOpen,
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected server response: {0}")]
    Protocol(String),
}

impl CalendarError {
    /// Errors worth one reconnect + retry: an expired session, or a keep-alive
    /// socket the server closed on its side (Exchange drops idle connections
    /// without a TLS close_notify, which surfaces as a network error on the
    /// next request). A retry re-runs auth on a fresh connection. Protocol and
    /// config errors are not retried — retrying would not change them.
    pub fn is_retryable(&self) -> bool {
        matches!(self, CalendarError::AuthFailed | CalendarError::Network(_))
    }
}

/// Parses the two date shapes OWA emits. Everything is treated as UTC: we ask
/// for `TimeZoneContext.Id = "UTC"`, so a naive `2026-09-04T10:00:00` (no `Z`)
/// really is UTC — copying the reference's `and_utc()` on a *local* time is
/// exactly the bug that shifts every meeting by the offset.
pub fn parse_owa_date(s: &str) -> Option<DateTime<Utc>> {
    if let Some(rest) = s.strip_prefix("/Date(") {
        let digits: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect();
        return digits.parse::<i64>().ok().and_then(DateTime::from_timestamp_millis);
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|n| n.and_utc())
}

/// Validate + normalize a URL for opening. Only `https` with a real host is
/// allowed; a bare host is upgraded to `https://` (that is the *only*
/// synthesis we do). `http`, `file:`, `javascript:`, `data:`, custom schemes
/// and empty hosts are rejected. Deliberately NOT applied to arbitrary text —
/// only to a URL a `detect_meeting_url` pattern already matched.
pub fn safe_url(raw: &str) -> Option<String> {
    let t = raw.trim();
    let lower = t.to_lowercase();
    if lower.contains("://") && !lower.starts_with("https://") {
        return None; // http, ftp, custom app schemes — refuse
    }
    let candidate = if lower.starts_with("https://") {
        t.to_string()
    } else {
        format!("https://{t}")
    };
    let url = url::Url::parse(&candidate).ok()?;
    if url.scheme() == "https" && url.host_str().is_some_and(|h| !h.is_empty()) {
        Some(candidate)
    } else {
        None
    }
}

/// Meeting-link platforms, each anchored at `https://` so a random substring
/// in a meeting body (`evil.com/x#foo.ktalk.ru`) cannot be turned into a URL
/// we then open — that unanchored-substring hole is why the reference detector
/// is unsafe. Returns the trimmed URL (trailing punctuation stripped) + the
/// platform. `Generic` catches any other `https://` link.
pub fn detect_meeting_url(text: &str) -> Option<(String, MeetingPlatform)> {
    // ponytail: only https:// links are detected; schemeless meeting links in
    // bodies won't get a Join button. Loosen per-platform if it ever matters.
    for (re, platform) in PATTERNS.iter() {
        if let Some(m) = re.find(text) {
            let url = m
                .as_str()
                .trim_end_matches(|c: char| ".,;\"')]>".contains(c))
                .to_string();
            if let Some(safe) = safe_url(&url) {
                return Some((safe, *platform));
            }
        }
    }
    None
}

use regex::Regex;
use std::sync::LazyLock;

static PATTERNS: LazyLock<Vec<(Regex, MeetingPlatform)>> = LazyLock::new(|| {
    use MeetingPlatform::*;
    let p = |s: &str| Regex::new(s).expect("static meeting-url regex");
    vec![
        (p(r#"https://teams\.microsoft\.com/l/meetup-join/[^\s<"')\]]+"#), Teams),
        (p(r#"https://teams\.live\.com/meet/[^\s<"')\]]+"#), Teams),
        (p(r#"https://[a-z0-9-]+\.zoom\.us/j/[^\s<"')\]]+"#), Zoom),
        (p(r#"https://meet\.google\.com/[a-z]{3}-[a-z]{4}-[a-z]{3}[^\s<"')\]]*"#), GoogleMeet),
        (p(r#"https://[a-z0-9-]+\.webex\.com/(?:meet|j|wc)/[^\s<"')\]]+"#), Webex),
        (p(r#"https://telemost\.yandex\.ru/j/[^\s<"')\]]+"#), Telemost),
        (p(r#"https://jazz\.sber\.ru/[^\s<"')\]]+"#), Jazz),
        (p(r#"https://vk\.com/call/join/[^\s<"')\]]+"#), Vk),
        (p(r#"https://[a-z0-9-]+\.ktalk\.ru/[^\s<"')\]]+"#), Ktalk),
        (p(r#"https://[a-z0-9-]+\.mts-link\.ru/j/[^\s<"')\]]+"#), MtsLink),
        // Fallback: any other https link (e.g. a JoinOnlineMeetingUrl the
        // server filled in directly). Last so specific hosts win.
        (p(r#"https://[^\s<"')\]]+"#), Generic),
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_utc_not_shifted() {
        // Naive (what OWA returns when asked in UTC) must be read AS utc.
        assert_eq!(
            parse_owa_date("2026-09-04T10:00:00").unwrap().to_rfc3339(),
            "2026-09-04T10:00:00+00:00"
        );
        // Explicit Z.
        assert_eq!(
            parse_owa_date("2026-09-04T07:00:00Z").unwrap().to_rfc3339(),
            "2026-09-04T07:00:00+00:00"
        );
        // WCF /Date(ms)/ — 1725444000000 = 2024-09-04T10:00:00Z.
        assert_eq!(
            parse_owa_date("/Date(1725444000000)/").unwrap().to_rfc3339(),
            "2024-09-04T10:00:00+00:00"
        );
        assert!(parse_owa_date("garbage").is_none());
    }

    #[test]
    fn safe_url_only_https() {
        assert_eq!(safe_url("https://a.ru/x").as_deref(), Some("https://a.ru/x"));
        assert_eq!(safe_url("telemost.yandex.ru/j/1").as_deref(), Some("https://telemost.yandex.ru/j/1"));
        assert!(safe_url("http://a.ru").is_none()); // not silently upgraded
        assert!(safe_url("file:///etc/passwd").is_none());
        assert!(safe_url("javascript:alert(1)").is_none());
        assert!(safe_url("https://").is_none()); // empty host
    }

    #[test]
    fn detects_each_platform() {
        let cases = [
            ("join: https://telemost.yandex.ru/j/123456.", MeetingPlatform::Telemost),
            ("https://us02web.zoom.us/j/999?pwd=x", MeetingPlatform::Zoom),
            ("https://teams.microsoft.com/l/meetup-join/19%3ameet", MeetingPlatform::Teams),
            ("https://meet.google.com/abc-defg-hij", MeetingPlatform::GoogleMeet),
            ("https://my.ktalk.ru/1234567", MeetingPlatform::Ktalk),
            ("see https://intranet.company.ru/room/5", MeetingPlatform::Generic),
        ];
        for (text, want) in cases {
            let (_, got) = detect_meeting_url(text).unwrap_or_else(|| panic!("no match: {text}"));
            assert_eq!(got, want, "for {text}");
        }
    }

    #[test]
    fn detector_does_not_synthesize_from_substring() {
        // The #5 hole: a bare host mentioned in text must NOT become a URL.
        assert!(detect_meeting_url("evil.com/x#foo.ktalk.ru").is_none());
    }

    #[test]
    fn base_url_validation() {
        assert!(validate_base_url("").is_ok()); // empty → use preset
        assert!(validate_base_url("https://owa.company.ru").is_ok());
        assert!(validate_base_url("  https://owa.company.ru/  ").is_ok());
        assert!(validate_base_url("http://owa.company.ru").is_err()); // not https
        assert!(validate_base_url("https://10.0.0.5").is_err()); // IP literal
        assert!(validate_base_url("https://[::1]").is_err()); // IPv6 literal
        assert!(validate_base_url("ftp://x").is_err());
        assert!(validate_base_url("owa.company.ru").is_err()); // no scheme
    }

    #[test]
    fn circuit_breaker_opens_after_two_failures() {
        let mut b = CircuitBreaker::default();
        assert!(!b.is_open());
        assert!(!b.record_auth_failure()); // 1st — still closed
        assert!(!b.is_open());
        assert!(b.record_auth_failure()); // 2nd — trips
        assert!(b.is_open());
    }

    #[test]
    fn circuit_breaker_success_and_reset_clear_it() {
        let mut b = CircuitBreaker::default();
        b.record_auth_failure();
        b.record_success();
        assert!(!b.is_open());
        assert_eq!(b.failures, 0);

        b.record_auth_failure();
        b.record_auth_failure();
        assert!(b.is_open());
        b.reset();
        assert!(!b.is_open());
    }

    #[test]
    fn reminder_fires_inside_the_lead_window_only() {
        let base = CalendarEvent {
            id: "1".into(), change_key: None, title: "Standup".into(),
            start: parse_owa_date("2026-09-04T10:00:00").unwrap(),
            end: parse_owa_date("2026-09-04T10:30:00").unwrap(),
            is_all_day: false, is_cancelled: false, is_organizer: false,
            organizer: None, location: None, join_url: None,
            platform: MeetingPlatform::Generic,
            response_type: MeetingResponseType::Accepted,
        };
        let at = |t: &str| parse_owa_date(t).unwrap();

        assert_eq!(reminder_due_in(&base, 5, at("2026-09-04T09:56:00")), Some(4));
        assert_eq!(reminder_due_in(&base, 5, at("2026-09-04T09:54:00")), None); // too early
        assert_eq!(reminder_due_in(&base, 5, at("2026-09-04T10:00:00")), None); // under way
        assert_eq!(reminder_due_in(&base, 0, at("2026-09-04T09:56:00")), None); // off
        // Never rounds down to a "через 0 мин" that reads as a bug.
        assert_eq!(reminder_due_in(&base, 5, at("2026-09-04T09:59:50")), Some(1));

        let mut declined = base.clone();
        declined.response_type = MeetingResponseType::Declined;
        assert_eq!(reminder_due_in(&declined, 5, at("2026-09-04T09:56:00")), None);

        let mut all_day = base.clone();
        all_day.is_all_day = true;
        assert_eq!(reminder_due_in(&all_day, 5, at("2026-09-04T09:56:00")), None);

        let mut cancelled = base.clone();
        cancelled.title = "Отменено: Standup".into();
        assert_eq!(reminder_due_in(&cancelled, 5, at("2026-09-04T09:56:00")), None);
    }
}

