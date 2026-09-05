//! The calendar's live state, owned by Rust because the dock panel unmounts
//! when hidden: the session, the event cache, and the circuit breaker all
//! live here for the process lifetime (managed as `Arc<CalendarState>`), so a
//! hidden panel keeps its data and a background tick can refresh it.
//!
//! The breaker is the AD-lockout guard: two consecutive auth failures open it
//! and drop the session, and nothing retries automatically until the user
//! re-enters the password (`set_password`), which is the deliberate "retry
//! login" action.
//!
//! ponytail: no event-sink port — the single caller that emits to the UI is
//! the background loop in `lib.rs` (app layer, allowed an `AppHandle`), so a
//! `Arc<dyn Fn(Event)>` indirection would be one impl for one caller. Add it
//! if a second emitter appears.

use std::sync::Mutex;

use chrono::{Duration, Utc};

use crate::domain::calendar::{CalendarError, CalendarEvent, CalendarStatus, CircuitBreaker};
use crate::infra::{owa_client::OwaSession, owa_credentials_store};
use crate::services::calendar_config;

#[derive(Default)]
struct Inner {
    session: Option<OwaSession>,
    cache: Vec<CalendarEvent>,
    breaker: CircuitBreaker,
    last_error: Option<String>,
}

#[derive(Default)]
pub struct CalendarState {
    inner: Mutex<Inner>,
}

impl CalendarState {
    pub fn new() -> Self {
        Self::default()
    }

    /// The UTC window the cache covers: yesterday through eight days out —
    /// wide enough that any viewer timezone sees "today + next 7 days", with
    /// the frontend slicing/grouping to its own day boundaries.
    fn window() -> (chrono::DateTime<Utc>, chrono::DateTime<Utc>) {
        let now = Utc::now();
        (now - Duration::days(1), now + Duration::days(8))
    }

    /// Sets the password for this run and (re)builds the session. Stores it
    /// only when `remember`. Always resets the breaker — this is the manual
    /// "retry login". Returns the settings/config error if not addressable.
    pub fn set_password(&self, password: &str, remember: bool) -> Result<(), CalendarError> {
        let session = calendar_config::build_session(password).ok_or(CalendarError::NotConfigured)??;
        if remember {
            owa_credentials_store::save_password(password).map_err(CalendarError::Network)?;
        }
        let mut inner = self.lock();
        inner.session = Some(session);
        inner.breaker.reset();
        inner.last_error = None;
        Ok(())
    }

    /// Drops the live session and any remembered password.
    pub fn forget(&self) -> Result<(), CalendarError> {
        owa_credentials_store::delete_password().map_err(CalendarError::Network)?;
        let mut inner = self.lock();
        inner.session = None;
        Ok(())
    }

    /// Builds a session from a remembered password if we don't have one yet.
    /// Silent when nothing is remembered — the panel then prompts for it.
    fn ensure_session(&self, inner: &mut Inner) {
        if inner.session.is_some() {
            return;
        }
        if let Some(pw) = owa_credentials_store::get_password() {
            if let Some(Ok(session)) = calendar_config::build_session(&pw) {
                inner.session = Some(session);
            }
        }
    }

    /// Fetches the window and refreshes the cache. Honors the breaker, and
    /// trips it on repeated auth failure. Blocking — callers use spawn_blocking.
    pub fn sync(&self) -> Result<Vec<CalendarEvent>, CalendarError> {
        let mut inner = self.lock();
        if inner.breaker.is_open() {
            return Err(CalendarError::CircuitOpen);
        }
        self.ensure_session(&mut inner);
        let session = inner.session.as_mut().ok_or(CalendarError::MissingPassword)?;

        let (start, end) = Self::window();
        match session.fetch_calendar_view(start, end) {
            Ok(events) => {
                inner.breaker.record_success();
                inner.last_error = None;
                inner.cache = events.clone();
                Ok(events)
            }
            Err(CalendarError::AuthFailed) => {
                inner.last_error = Some(CalendarError::AuthFailed.to_string());
                if inner.breaker.record_auth_failure() {
                    inner.session = None; // breaker open — stop using bad credentials
                }
                Err(CalendarError::AuthFailed)
            }
            Err(other) => {
                inner.last_error = Some(other.to_string());
                Err(other)
            }
        }
    }

    /// Fetches attendees + body for one meeting. Does not affect the breaker
    /// (the periodic sync governs that); it just needs a live session.
    pub fn event_details(
        &self,
        id: &str,
        change_key: Option<&str>,
    ) -> Result<crate::domain::calendar::EventDetails, CalendarError> {
        let mut inner = self.lock();
        if inner.breaker.is_open() {
            return Err(CalendarError::CircuitOpen);
        }
        self.ensure_session(&mut inner);
        let session = inner.session.as_mut().ok_or(CalendarError::MissingPassword)?;
        session.fetch_event_details(id, change_key)
    }

    /// Sends an RSVP and optimistically updates the cached event's response so
    /// the timeline reflects it immediately (the next sync confirms it).
    pub fn rsvp(
        &self,
        action: crate::domain::calendar::RsvpAction,
        id: &str,
        change_key: &str,
        send: bool,
    ) -> Result<(), CalendarError> {
        let mut inner = self.lock();
        if inner.breaker.is_open() {
            return Err(CalendarError::CircuitOpen);
        }
        self.ensure_session(&mut inner);
        {
            let session = inner.session.as_mut().ok_or(CalendarError::MissingPassword)?;
            session.rsvp(action, id, change_key, send)?;
        }
        if let Some(ev) = inner.cache.iter_mut().find(|e| e.id == id) {
            ev.response_type = action.resulting_response();
        }
        Ok(())
    }

    pub fn cached(&self) -> Vec<CalendarEvent> {
        self.lock().cache.clone()
    }

    pub fn status(&self) -> CalendarStatus {
        let inner = self.lock();
        CalendarStatus {
            configured: calendar_config::effective()
                .map(|s| !s.username.trim().is_empty())
                .unwrap_or(false),
            connected: inner.session.is_some() || owa_credentials_store::has_password(),
            circuit_open: inner.breaker.is_open(),
            cached_count: inner.cache.len(),
            last_error: inner.last_error.clone(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}
