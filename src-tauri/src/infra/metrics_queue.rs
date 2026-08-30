//! Disk-backed queue of metric events awaiting delivery —
//! `~/.atlas/metrics_queue.db`. Shape follows `infra::tool_call_log`: a
//! fresh connection per call, schema as `CREATE TABLE IF NOT EXISTS`,
//! opportunistic retention on write, best-effort throughout.
//!
//! Why a queue at all: the collector is only reachable on the corporate
//! network, and a desktop app is regularly off it. With one event per
//! install that was survivable — the send simply retried next launch. From
//! the second event onwards it is not: everything that happened offline
//! would be lost, and the loss would be silently biased towards exactly
//! the people who work outside the office.
//!
//! Rows hold the event's *field map*, not the whole envelope, so a flush
//! can post everything pending as one `payload_data` array instead of one
//! request per event.

use std::path::PathBuf;

use rusqlite::{params, Connection};
use serde_json::Value;
use thiserror::Error;

const DB_FILE_NAME: &str = "metrics_queue.db";

/// Events older than this are dropped unsent. A month-old "app started"
/// is not worth reporting, and keeping it would let a permanently offline
/// install grow the file without bound.
const RETENTION_DAYS: i64 = 14;

/// Hard ceiling regardless of age, oldest dropped first. Guards the
/// pathological case: heavy daily use with no network access at all.
const MAX_QUEUED: i64 = 1000;

/// Per-flush cap. Keeps one request bounded when a large backlog finally
/// reaches the network; the rest goes out on the next flush.
const FLUSH_BATCH: i64 = 50;

const SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 3000;

CREATE TABLE IF NOT EXISTS pending_events (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  ts_ms        INTEGER NOT NULL,
  payload_json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_pending_events_ts ON pending_events(ts_ms);
"#;

#[derive(Debug, Error)]
pub enum MetricsQueueError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings error: {0}")]
    Settings(#[from] crate::domain::settings::SettingsError),
    #[error("serialize error: {0}")]
    Serialize(#[from] serde_json::Error),
}

fn db_path() -> Result<PathBuf, MetricsQueueError> {
    Ok(crate::infra::settings_store::settings_dir()?.join(DB_FILE_NAME))
}

fn open() -> Result<Connection, MetricsQueueError> {
    let path = db_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}

/// Appends one built event field map, then prunes by age and by count.
pub fn enqueue(fields: &Value, now_ms: i64) -> Result<(), MetricsQueueError> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO pending_events (ts_ms, payload_json) VALUES (?1, ?2)",
        params![now_ms, serde_json::to_string(fields)?],
    )?;
    prune(&conn, now_ms)?;
    Ok(())
}

fn prune(conn: &Connection, now_ms: i64) -> Result<(), MetricsQueueError> {
    let cutoff = now_ms - RETENTION_DAYS * 24 * 60 * 60 * 1000;
    conn.execute("DELETE FROM pending_events WHERE ts_ms < ?1", params![cutoff])?;
    conn.execute(
        "DELETE FROM pending_events WHERE id NOT IN \
         (SELECT id FROM pending_events ORDER BY id DESC LIMIT ?1)",
        params![MAX_QUEUED],
    )?;
    Ok(())
}

/// The oldest pending events, with their row ids, up to `FLUSH_BATCH`.
/// A row whose JSON no longer parses is dropped rather than blocking the
/// queue behind it forever.
pub fn take_batch() -> Result<Vec<(i64, Value)>, MetricsQueueError> {
    let conn = open()?;
    let mut stmt =
        conn.prepare("SELECT id, payload_json FROM pending_events ORDER BY id ASC LIMIT ?1")?;
    let rows = stmt
        .query_map(params![FLUSH_BATCH], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut out = Vec::with_capacity(rows.len());
    let mut corrupt = Vec::new();
    for (id, json) in rows {
        match serde_json::from_str::<Value>(&json) {
            Ok(value) => out.push((id, value)),
            Err(_) => corrupt.push(id),
        }
    }
    if !corrupt.is_empty() {
        delete(&conn, &corrupt)?;
    }
    Ok(out)
}

/// Removes rows the collector has confirmed. Called only after a 2xx, so
/// a failed flush leaves everything in place for the next attempt.
pub fn delete_confirmed(ids: &[i64]) -> Result<(), MetricsQueueError> {
    if ids.is_empty() {
        return Ok(());
    }
    let conn = open()?;
    delete(&conn, ids)
}

fn delete(conn: &Connection, ids: &[i64]) -> Result<(), MetricsQueueError> {
    let mut stmt = conn.prepare("DELETE FROM pending_events WHERE id = ?1")?;
    for id in ids {
        stmt.execute(params![id])?;
    }
    Ok(())
}

/// Test accessor: the queue has no runtime consumer for its own depth —
/// nothing branches on how much is pending, it either drains or waits.
#[cfg(test)]
pub fn pending_count() -> Result<i64, MetricsQueueError> {
    let conn = open()?;
    Ok(conn.query_row("SELECT COUNT(*) FROM pending_events", [], |row| row.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;
    use serde_json::json;

    fn event(tag: &str) -> Value {
        json!({ "e": "se", "se_la": tag })
    }

    const DAY_MS: i64 = 24 * 60 * 60 * 1000;

    #[test]
    fn enqueued_events_come_back_oldest_first() {
        with_temp_home(|| {
            enqueue(&event("a"), 1_000).unwrap();
            enqueue(&event("b"), 2_000).unwrap();

            let batch = take_batch().unwrap();
            assert_eq!(batch.len(), 2);
            assert_eq!(batch[0].1["se_la"], "a");
            assert_eq!(batch[1].1["se_la"], "b");
        });
    }

    #[test]
    fn only_confirmed_rows_are_removed() {
        with_temp_home(|| {
            enqueue(&event("a"), 1_000).unwrap();
            enqueue(&event("b"), 2_000).unwrap();

            let batch = take_batch().unwrap();
            delete_confirmed(&[batch[0].0]).unwrap();

            let left = take_batch().unwrap();
            assert_eq!(left.len(), 1);
            assert_eq!(left[0].1["se_la"], "b");
        });
    }

    /// The whole point of the queue: a failed flush must lose nothing.
    #[test]
    fn a_flush_that_confirms_nothing_leaves_the_queue_intact() {
        with_temp_home(|| {
            enqueue(&event("a"), 1_000).unwrap();
            enqueue(&event("b"), 2_000).unwrap();

            let _batch = take_batch().unwrap();
            delete_confirmed(&[]).unwrap();

            assert_eq!(pending_count().unwrap(), 2);
        });
    }

    #[test]
    fn events_older_than_the_retention_window_are_dropped() {
        with_temp_home(|| {
            let now = 100 * DAY_MS;
            enqueue(&event("ancient"), now - (RETENTION_DAYS + 1) * DAY_MS).unwrap();
            enqueue(&event("fresh"), now).unwrap();

            let batch = take_batch().unwrap();
            assert_eq!(batch.len(), 1, "the stale event must not survive");
            assert_eq!(batch[0].1["se_la"], "fresh");
        });
    }

    #[test]
    fn the_queue_is_capped_and_drops_the_oldest_first() {
        with_temp_home(|| {
            for i in 0..(MAX_QUEUED + 5) {
                enqueue(&event(&format!("e{i}")), 1_000 + i).unwrap();
            }
            assert_eq!(pending_count().unwrap(), MAX_QUEUED);

            let batch = take_batch().unwrap();
            assert_eq!(
                batch[0].1["se_la"], "e5",
                "the five oldest events should have been dropped"
            );
        });
    }

    #[test]
    fn a_flush_is_capped_so_a_backlog_goes_out_in_chunks() {
        with_temp_home(|| {
            for i in 0..(FLUSH_BATCH + 10) {
                enqueue(&event(&format!("e{i}")), 1_000 + i).unwrap();
            }
            assert_eq!(take_batch().unwrap().len() as i64, FLUSH_BATCH);
        });
    }

    #[test]
    fn a_corrupt_row_is_dropped_instead_of_blocking_the_queue() {
        with_temp_home(|| {
            enqueue(&event("good"), 1_000).unwrap();
            {
                let conn = open().unwrap();
                conn.execute(
                    "INSERT INTO pending_events (ts_ms, payload_json) VALUES (?1, ?2)",
                    params![500_i64, "{not json"],
                )
                .unwrap();
            }

            let batch = take_batch().unwrap();
            assert_eq!(batch.len(), 1);
            assert_eq!(batch[0].1["se_la"], "good");
            assert_eq!(pending_count().unwrap(), 1, "the corrupt row must be gone");
        });
    }
}
