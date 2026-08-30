//! Identity and clock of one app run.
//!
//! Deliberately process-local and never written to disk: a session id is
//! meaningful only while the app is running, and persisting it would turn
//! a throwaway grouping key into a second durable identifier.
//!
//! Two clocks, not one. Wall-clock time from launch to exit is nearly
//! meaningless for a desktop tool that people leave open all day — it
//! would report an eight-hour "session" for an app that sat behind a
//! browser the whole time. What answers "was this used" is the time the
//! window actually had focus, accumulated across every focus/blur cycle.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Focus {
    /// When the current focused stretch began, if the window has focus.
    since: Option<Instant>,
    /// Focused stretches already closed out.
    total: Duration,
}

struct Session {
    id: String,
    started: Instant,
    focus: Mutex<Focus>,
}

static SESSION: OnceLock<Session> = OnceLock::new();

fn session() -> &'static Session {
    SESSION.get_or_init(|| Session {
        id: uuid::Uuid::new_v4().to_string(),
        started: Instant::now(),
        focus: Mutex::new(Focus::default()),
    })
}

pub fn id() -> &'static str {
    &session().id
}

/// Seconds since launch, focused or not. `Instant` rather than wall-clock
/// time so a clock change or a suspend/resume can't produce a negative or
/// absurd duration.
pub fn elapsed_secs() -> f64 {
    session().started.elapsed().as_secs_f64()
}

/// Called from the window's focus events. Idempotent in both directions:
/// some platforms deliver a repeated focus or blur, and double-counting a
/// stretch would inflate active time.
pub fn set_focused(focused: bool) {
    let session = session();
    let mut focus = session.focus.lock().unwrap_or_else(|e| e.into_inner());
    match (focused, focus.since) {
        (true, None) => focus.since = Some(Instant::now()),
        (false, Some(since)) => {
            focus.total += since.elapsed();
            focus.since = None;
        }
        _ => {}
    }
}

/// Seconds the window has actually had focus, including the stretch in
/// progress.
pub fn active_secs() -> f64 {
    let session = session();
    let focus = session.focus.lock().unwrap_or_else(|e| e.into_inner());
    let open = focus.since.map(|s| s.elapsed()).unwrap_or_default();
    (focus.total + open).as_secs_f64()
}

static END_RECORDED: AtomicBool = AtomicBool::new(false);

/// Active and total seconds, returned **once**. Shutdown can arrive
/// through more than one path (the window's close request, the explicit
/// exit command, and on macOS both), and each would otherwise record its
/// own session-end event, inflating the session count and halving the
/// average length.
pub fn take_end_secs() -> Option<(f64, f64)> {
    if END_RECORDED.swap(true, Ordering::SeqCst) {
        return None;
    }
    Some((active_secs(), elapsed_secs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_id_is_stable_within_a_run() {
        assert_eq!(id(), id());
        assert_eq!(uuid::Uuid::parse_str(id()).unwrap().to_string(), id());
    }

    #[test]
    fn elapsed_never_goes_backwards() {
        let first = elapsed_secs();
        let second = elapsed_secs();
        assert!(second >= first);
        assert!(first >= 0.0);
    }

    /// Focus bookkeeping is exercised on a local `Focus` rather than the
    /// process-wide session: the real one is a `OnceLock` shared with
    /// every other test in the binary, so driving it here would make those
    /// order-dependent.
    #[test]
    fn active_time_accumulates_only_across_focused_stretches() {
        let mut focus = Focus::default();

        // Blur before any focus is a no-op, not a negative stretch.
        assert!(focus.since.is_none());
        focus.total += Duration::from_secs(0);

        focus.since = Some(Instant::now() - Duration::from_secs(5));
        // Closing the stretch banks it.
        if let Some(since) = focus.since.take() {
            focus.total += since.elapsed();
        }
        assert!(focus.total.as_secs_f64() >= 5.0);

        // A second blur must not bank anything again.
        let banked = focus.total;
        if let Some(since) = focus.since.take() {
            focus.total += since.elapsed();
        }
        assert_eq!(focus.total, banked);
    }

    #[test]
    fn active_time_never_exceeds_wall_clock_time() {
        assert!(active_secs() <= elapsed_secs() + 0.5);
    }

    #[test]
    fn the_session_end_is_reported_at_most_once() {
        assert!(take_end_secs().is_some());
        assert!(
            take_end_secs().is_none(),
            "a second shutdown path must not record a second session"
        );
    }
}
