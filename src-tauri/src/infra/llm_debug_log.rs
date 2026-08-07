//! Optional, off-by-default logging of every request sent to and every
//! response (or error) received from an LLM provider — gated by
//! `LlmSettings.debug_logging`, toggled from the LLM settings tab. Off by
//! default since a conversation can contain sensitive document content the
//! user may not want written to disk.
//!
//! Appends JSON Lines to `~/.atlas/logs/llm.jsonl`, one entry per request
//! or response/error, each stamped with which tool-calling round (see
//! `commands::llm::llm_chat_stream`) it belongs to — so a provider error
//! (e.g. an opaque `traceId` in a 500 body) can be correlated with the
//! exact `ChatRequest` that produced it. Logs the already-typed
//! `ChatRequest`/`ChatStreamResult` domain values (not raw HTTP bytes) —
//! that's every field a request/response actually carries (messages,
//! tools, tool calls, usage), and avoids duplicating wire-format parsing
//! into a second, HTTP-client-specific logging path.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::domain::llm::{ChatRequest, ChatStreamResult, LlmError};
use crate::domain::settings::SettingsError;
use crate::infra::settings_store;

const LOG_FILE_NAME: &str = "llm.jsonl";

#[derive(Serialize)]
struct LogEntry {
    ts: u128,
    #[serde(rename = "providerId")]
    provider_id: String,
    round: u32,
    direction: &'static str,
    payload: serde_json::Value,
}

fn default_log_path() -> Result<PathBuf, SettingsError> {
    Ok(settings_store::settings_dir()?.join("logs").join(LOG_FILE_NAME))
}

/// Logs the `ChatRequest` about to be sent for one tool-calling round.
/// No-op when `enabled` is `false` — callers pass `settings.llm.
/// debug_logging` straight through rather than checking it themselves.
pub fn log_request(enabled: bool, provider_id: &str, round: u32, request: &ChatRequest) {
    if !enabled {
        return;
    }
    let Ok(path) = default_log_path() else { return };
    append(&path, provider_id, round, "request", request);
}

/// Logs the outcome of one tool-calling round — the `ChatStreamResult` on
/// success, or the `LlmError`'s message (already includes the response
/// body for an HTTP status error, see `openai_compatible::
/// ok_or_status_error`) on failure.
pub fn log_response(enabled: bool, provider_id: &str, round: u32, result: &Result<ChatStreamResult, LlmError>) {
    if !enabled {
        return;
    }
    let Ok(path) = default_log_path() else { return };
    match result {
        Ok(response) => append(&path, provider_id, round, "response", response),
        Err(error) => append(&path, provider_id, round, "error", &error.to_string()),
    }
}

/// Best-effort only — a logging failure (disk full, permissions) must
/// never break a real chat turn, so every fallible step here silently
/// drops the entry rather than propagating an error. `path` is a parameter
/// (rather than always resolving `default_log_path()` internally)
/// specifically so tests can point this at a throwaway file instead of the
/// real `~/.atlas/logs/llm.jsonl`.
fn append<T: Serialize>(path: &Path, provider_id: &str, round: u32, direction: &'static str, payload: &T) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else { return };
    let Ok(payload) = serde_json::to_value(payload) else { return };
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let entry = LogEntry { ts, provider_id: provider_id.to_string(), round, direction, payload };
    if let Ok(line) = serde_json::to_string(&entry) {
        let _ = writeln!(file, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::llm::{LlmMessage, LlmRole};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime as St, UNIX_EPOCH as Ep};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A throwaway `.jsonl` path under the OS temp dir — never the real
    /// `~/.atlas/logs/llm.jsonl` — so these tests exercise the real file
    /// I/O in `append` without touching global process state (no `HOME`
    /// mutation, safe under `cargo test`'s parallel execution).
    fn fixture_path() -> PathBuf {
        let nanos = St::now().duration_since(Ep).unwrap().as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("alfa-atlas-llm-debug-log-{nanos}-{n}")).join("llm.jsonl")
    }

    fn sample_request() -> ChatRequest {
        ChatRequest {
            messages: vec![LlmMessage {
                role: LlmRole::User,
                content: Some("hi".to_string()),
                tool_call_id: None,
                tool_calls: vec![],
            }],
            tools: vec![],
            model: "gpt-4o-mini".to_string(),
        }
    }

    #[test]
    fn append_creates_parent_dir_and_writes_one_json_line() {
        let path = fixture_path();
        append(&path, "alfagen", 1, "request", &sample_request());

        let contents = std::fs::read_to_string(&path).unwrap();
        let line: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(line["providerId"], "alfagen");
        assert_eq!(line["round"], 1);
        assert_eq!(line["direction"], "request");
        assert_eq!(line["payload"]["model"], "gpt-4o-mini");

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn append_response_entry_carries_the_result_payload() {
        let path = fixture_path();
        let result = ChatStreamResult { text: "hello".to_string(), usage: None, tool_calls: vec![] };
        append(&path, "alfagen", 1, "response", &result);

        let contents = std::fs::read_to_string(&path).unwrap();
        let line: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(line["direction"], "response");
        assert_eq!(line["payload"]["text"], "hello");

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn append_error_entry_carries_the_error_message_as_a_string_payload() {
        let path = fixture_path();
        append(&path, "alfagen", 2, "error", &"http status 500: boom".to_string());

        let contents = std::fs::read_to_string(&path).unwrap();
        let line: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(line["direction"], "error");
        assert_eq!(line["round"], 2);
        assert_eq!(line["payload"], "http status 500: boom");

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn multiple_appends_produce_separate_lines_in_order() {
        let path = fixture_path();
        append(&path, "alfagen", 1, "request", &sample_request());
        append(&path, "alfagen", 2, "request", &sample_request());

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(first["round"], 1);
        assert_eq!(second["round"], 2);

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn log_request_is_a_no_op_when_disabled() {
        // Exercises the real public entry point (not just `append`) to
        // confirm the `enabled` gate short-circuits before any file I/O —
        // uses `default_log_path()` internally, so this only checks that
        // nothing panics and no path resolution/write is attempted; the
        // gate itself is what's under test, not where the file would land.
        log_request(false, "alfagen", 1, &sample_request());
    }
}
