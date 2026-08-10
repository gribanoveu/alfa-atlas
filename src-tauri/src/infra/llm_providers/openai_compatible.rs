//! OpenAI-compatible chat-completions client — mirrors
//! `infra::embedding_providers::remote`'s shape closely. TLS trust
//! configuration (a corporate internal CA, for a provider like AlfaGen) is
//! configured per-`Agent` via `infra::http_agent::build_agent`.
//!
//! Blocking HTTP (`ureq`), not `reqwest`, same reasoning as the embeddings
//! sibling: this project's `tokio` dependency only enables
//! `sync, rt, macros, time` (no `net`), and a request-per-call blocking
//! client doesn't justify expanding that.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::llm::{
    ChatRequest, ChatResponse, ChatStreamResult, ChatUsage, LlmError, LlmModelInfo, LlmProvider,
    LlmRole, LlmToolCall,
};
use crate::infra::http_agent;

/// Thin wrapper around `http_agent::build_agent` that maps TLS failures
/// into `LlmError::Tls` — kept as a local helper so call sites and tests
/// in this module stay readable.
pub fn build_agent(trusted_cert_pem: Option<&str>) -> Result<ureq::Agent, LlmError> {
    http_agent::build_agent(trusted_cert_pem).map_err(|e| LlmError::Tls(e.0))
}

/// Cap on how many characters of an error response body get folded into
/// the error message — a provider's error page can be arbitrarily large
/// (an HTML error page, a stack trace), and the goal here is a diagnosable
/// message, not a full dump.
const ERROR_BODY_MAX_CHARS: usize = 2000;

/// Turns a non-2xx response into a detailed `LlmError::Http` that includes
/// the response body — `build_agent` disables ureq's default
/// `http_status_as_error` specifically so the body is still readable here
/// (ureq's own status-to-error conversion discards it). Returns the
/// response unchanged on a success status.
fn ok_or_status_error(
    mut response: http::Response<ureq::Body>,
) -> Result<http::Response<ureq::Body>, LlmError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response
        .body_mut()
        .read_to_string()
        .unwrap_or_else(|e| format!("<failed to read error response body: {e}>"));
    let truncated = if body.chars().count() > ERROR_BODY_MAX_CHARS {
        let head: String = body.chars().take(ERROR_BODY_MAX_CHARS).collect();
        format!("{head}… (truncated)")
    } else {
        body
    };
    Err(LlmError::Http(format!("http status {}: {}", status.as_u16(), truncated)))
}

fn role_str(role: LlmRole) -> &'static str {
    match role {
        LlmRole::System => "system",
        LlmRole::User => "user",
        LlmRole::Assistant => "assistant",
        LlmRole::Tool => "tool",
    }
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: Vec<WireMessage<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<WireTool<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'static str>,
    /// Omitted (not just `false`) for a non-streaming request — matches
    /// today's exact wire shape when unset, since some servers treat an
    /// explicit `"stream":false` differently from the key being absent.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
    /// Opts into a trailing `usage`-only SSE chunk on streaming requests —
    /// the standard OpenAI flag, without which many spec-compliant servers
    /// never emit `usage` at all. `None` for non-streaming requests, where
    /// `usage` is irrelevant (a plain `chat` response doesn't stream chunks
    /// to attach it to).
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Debug, Serialize)]
struct WireMessage<'a> {
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<WireToolCallOut<'a>>,
}

/// Outgoing shape of one tool call an assistant message previously
/// requested — round-tripped back to the provider so it sees its own prior
/// turn. Distinct from `WireToolCall`/`WireToolCallFunction` below (those
/// are the *incoming*, non-streaming response shape) purely because
/// `Serialize`/`Deserialize` want their fields borrowed vs. owned
/// differently; the wire JSON shape is otherwise identical.
#[derive(Debug, Serialize)]
struct WireToolCallOut<'a> {
    id: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
    function: WireToolCallFunctionOut<'a>,
}

#[derive(Debug, Serialize)]
struct WireToolCallFunctionOut<'a> {
    name: &'a str,
    arguments: &'a str,
}

#[derive(Debug, Serialize)]
struct WireTool<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    function: WireFunction<'a>,
}

#[derive(Debug, Serialize)]
struct WireFunction<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a Value,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<WireChoice>,
}

#[derive(Debug, Deserialize)]
struct WireChoice {
    message: WireResponseMessage,
}

#[derive(Debug, Deserialize)]
struct WireResponseMessage {
    #[serde(default)]
    content: Option<String>,
    // `#[serde(default)]` alone only covers a *missing* key — many
    // OpenAI-compatible servers, once `tools` is present on the request,
    // explicitly send `"tool_calls":null` on a turn that didn't request
    // any, which `Vec<T>`'s own `Deserialize` rejects ("invalid type:
    // null, expected a sequence"). `deserialize_null_default` treats an
    // explicit `null` the same as a missing key.
    #[serde(default, deserialize_with = "deserialize_null_default")]
    tool_calls: Vec<WireToolCall>,
}

#[derive(Debug, Deserialize)]
struct WireToolCall {
    id: String,
    function: WireToolCallFunction,
}

/// `arguments` stays a raw JSON-encoded string here, exactly as the wire
/// format carries it (a JSON object serialized *as a string*, not nested)
/// — no second parse happens in this layer, see `domain::llm::LlmToolCall`.
#[derive(Debug, Deserialize)]
struct WireToolCallFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct ModelsListResponse {
    data: Vec<ModelsListDatum>,
}

#[derive(Debug, Deserialize)]
struct ModelsListDatum {
    id: String,
}

#[derive(Debug, Default, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    // See `WireResponseMessage.tool_calls`'s comment — a content-only delta
    // chunk commonly carries an explicit `"tool_calls":null` once `tools`
    // was offered on the request, which plain `#[serde(default)]` doesn't
    // cover (that only fills in a *missing* key, not an explicit `null`).
    #[serde(default, deserialize_with = "deserialize_null_default")]
    tool_calls: Vec<StreamToolCallDelta>,
}

/// Treats an explicit JSON `null` the same as a missing key — plain
/// `#[serde(default)]` only covers the latter, and `Vec<T>`'s own
/// `Deserialize` rejects `null` outright ("invalid type: null, expected a
/// sequence"). Generic so both `WireResponseMessage.tool_calls` and
/// `StreamDelta.tool_calls` share one implementation.
fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::deserialize(deserializer)?.unwrap_or_default())
}

/// One fragment of a streamed tool call. OpenAI sends `id`/`function.name`
/// only on the fragment where a given `index` first appears, then
/// `function.arguments` incrementally across however many further
/// fragments share that `index` — this type is the raw per-chunk fragment,
/// not yet merged; `ToolCallAccumulator` does the merging.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct StreamToolCallDelta {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamToolCallFunctionDelta>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
struct StreamToolCallFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
}

/// Wire shape of the trailing `usage`-only chunk requested via
/// `stream_options.include_usage` — snake_case, unlike `ChatUsage`'s
/// `camelCase` (that rename is for the frontend-facing side, not this
/// deserialize side), hence a distinct type rather than deserializing
/// straight into `ChatUsage`.
#[derive(Debug, Deserialize)]
struct StreamUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

impl From<StreamUsage> for ChatUsage {
    fn from(u: StreamUsage) -> Self {
        ChatUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        }
    }
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<StreamUsage>,
}

/// One piece of information extracted from a single raw line of an
/// OpenAI-compatible SSE chat-completions stream.
#[derive(Debug, PartialEq)]
enum SseLine {
    /// A `data: {...}` line. `delta` is `Some(text)` when this chunk's
    /// `delta.content` was present and non-null (`None` for e.g. a
    /// role-only first chunk, or the usage-only trailing chunk).  `usage` is
    /// `Some` only on that trailing chunk — a chunk can in principle carry
    /// both, so these are independent, not an either/or. `tool_calls` is
    /// the chunk's raw (unmerged) tool-call fragments, empty for a chunk
    /// that carries none — a chunk can carry both `delta` text and
    /// `tool_calls` fragments (the model can emit prose before invoking a
    /// tool in the same turn).
    Chunk { delta: Option<String>, usage: Option<ChatUsage>, tool_calls: Vec<StreamToolCallDelta> },
    /// The terminal `data: [DONE]` line.
    Done,
    /// Not a `data:` line — a blank event-separator line, or (defensively)
    /// anything else this protocol doesn't actually send.
    Ignore,
}

/// Pure, network-free parsing of one SSE line — kept separate from
/// `chat_stream` so it's directly unit-testable against fixed strings, the
/// same convention this file's other wire-shape tests already follow.
fn parse_sse_line(line: &str) -> Result<SseLine, LlmError> {
    // Some servers use CRLF line endings; `BufRead::lines()` only strips
    // the trailing `\n`, so a stray `\r` can survive into the payload.
    let line = line.trim_end_matches('\r');
    let Some(data) = line.strip_prefix("data:") else {
        return Ok(SseLine::Ignore);
    };
    let data = data.trim();
    if data.is_empty() {
        return Ok(SseLine::Ignore);
    }
    if data == "[DONE]" {
        return Ok(SseLine::Done);
    }
    let chunk: StreamChunk =
        serde_json::from_str(data).map_err(|e| LlmError::Provider(e.to_string()))?;
    let usage = chunk.usage.map(ChatUsage::from);
    let (delta, tool_calls) = match chunk.choices.into_iter().next() {
        Some(choice) => (choice.delta.content, choice.delta.tool_calls),
        None => (None, Vec::new()),
    };
    Ok(SseLine::Chunk { delta, usage, tool_calls })
}

/// Accumulates fragmented `delta.tool_calls` entries across an SSE stream,
/// keyed by the wire's own `index` — see `StreamToolCallDelta`'s doc
/// comment for why a call's `id`/`name` and `arguments` arrive split across
/// however many fragments share that index.
#[derive(Debug, Default)]
struct ToolCallAccumulator {
    // Insertion-order-preserving (a `Vec`, not a `HashMap`) so `finish()`
    // emits calls in the order the model started them — order matters when
    // this becomes one assistant message's `tool_calls` array.
    entries: Vec<(usize, PartialToolCall)>,
    // Counter for a fragment with no `index` at all (malformed, but
    // shouldn't happen against a spec-compliant server). Offset well past
    // any realistic real index so it can never collide with one.
    next_synthetic_index: usize,
}

#[derive(Debug, Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl ToolCallAccumulator {
    fn ingest(&mut self, fragments: Vec<StreamToolCallDelta>) {
        for frag in fragments {
            // A fragment missing `index` is treated as its own new entry
            // rather than guessed into whichever entry is last — merging
            // it wrong would corrupt an unrelated, still-accumulating
            // call's `arguments` string with unrelated text, which is
            // worse than emitting one extra malformed call: that call
            // simply fails `parse_tool_call`'s JSON parse downstream and
            // gets reported back to the model as a recoverable tool error.
            let index = frag.index.unwrap_or_else(|| {
                let synthetic = 1_000_000 + self.next_synthetic_index;
                self.next_synthetic_index += 1;
                synthetic
            });
            let entry = match self.entries.iter_mut().find(|(i, _)| *i == index) {
                Some((_, e)) => e,
                None => {
                    self.entries.push((index, PartialToolCall::default()));
                    &mut self.entries.last_mut().expect("just pushed").1
                }
            };
            if let Some(id) = frag.id {
                entry.id = id;
            }
            if let Some(function) = frag.function {
                if let Some(name) = function.name {
                    entry.name = name;
                }
                if let Some(args) = function.arguments {
                    entry.arguments.push_str(&args);
                }
            }
        }
    }

    fn finish(self) -> Vec<LlmToolCall> {
        self.entries
            .into_iter()
            .map(|(_, e)| LlmToolCall { id: e.id, name: e.name, arguments: e.arguments })
            .collect()
    }
}

pub struct OpenAiCompatibleProvider {
    agent: ureq::Agent,
    base_url: String,
    api_key: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(agent: ureq::Agent, base_url: String, api_key: String) -> Self {
        Self { agent, base_url, api_key }
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    fn models_url(&self) -> String {
        format!("{}/models", self.base_url.trim_end_matches('/'))
    }

    /// Shared request-body construction for both `chat` and `chat_stream` —
    /// only `stream` differs between them.
    fn build_body<'a>(&self, request: &'a ChatRequest, stream: bool) -> ChatCompletionRequest<'a> {
        let messages: Vec<WireMessage> = request
            .messages
            .iter()
            .map(|m| WireMessage {
                role: role_str(m.role),
                content: m.content.as_deref(),
                tool_call_id: m.tool_call_id.as_deref(),
                tool_calls: m
                    .tool_calls
                    .iter()
                    .map(|tc| WireToolCallOut {
                        id: &tc.id,
                        kind: "function",
                        function: WireToolCallFunctionOut { name: &tc.name, arguments: &tc.arguments },
                    })
                    .collect(),
            })
            .collect();
        let tools: Vec<WireTool> = request
            .tools
            .iter()
            .map(|t| WireTool {
                kind: "function",
                function: WireFunction {
                    name: &t.name,
                    description: &t.description,
                    parameters: &t.parameters,
                },
            })
            .collect();
        // Only send `tool_choice` when tools are actually offered — an
        // empty `tools` array with `tool_choice: "auto"` is a needless
        // (and, on some OpenAI-compatible servers, rejected) combination.
        let tool_choice = if tools.is_empty() { None } else { Some("auto") };
        let stream_options = stream.then_some(StreamOptions { include_usage: true });
        ChatCompletionRequest {
            model: &request.model,
            messages,
            tools,
            tool_choice,
            stream,
            stream_options,
        }
    }
}

impl LlmProvider for OpenAiCompatibleProvider {
    fn chat(&self, request: ChatRequest) -> Result<ChatResponse, LlmError> {
        let body = self.build_body(&request, false);

        let mut response = self
            .agent
            .post(self.chat_url())
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .send_json(&body)
            .map_err(|e| LlmError::Http(e.to_string()))?;
        response = ok_or_status_error(response)?;

        let parsed: ChatCompletionResponse = response
            .body_mut()
            .read_json()
            .map_err(|e| LlmError::Http(e.to_string()))?;

        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::Provider("empty choices array".to_string()))?;

        Ok(ChatResponse {
            content: choice.message.content,
            tool_calls: choice
                .message
                .tool_calls
                .into_iter()
                .map(|tc| LlmToolCall {
                    id: tc.id,
                    name: tc.function.name,
                    arguments: tc.function.arguments,
                })
                .collect(),
        })
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        on_delta: &dyn Fn(&str),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ChatStreamResult, LlmError> {
        let body = self.build_body(&request, true);

        let response = self
            .agent
            .post(self.chat_url())
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .send_json(&body)
            .map_err(|e| LlmError::Http(e.to_string()))?;
        let response = ok_or_status_error(response)?;

        let reader = std::io::BufReader::new(response.into_body().into_reader());
        let mut full = String::new();
        let mut usage = None;
        let mut tool_calls_acc = ToolCallAccumulator::default();
        for line in std::io::BufRead::lines(reader) {
            // Checked once per SSE line rather than only before the loop —
            // a chatty/looping model's response can take many seconds to
            // finish, and this is what lets a user-initiated stop
            // (`commands::llm::llm_cancel_chat`) land within roughly one
            // chunk of being requested instead of only once the whole
            // response has streamed. `lines()` itself still blocks
            // synchronously on the socket read for the *next* line, so a
            // connection that stalls entirely between chunks isn't helped
            // by this — see this method's doc comment on the trait.
            if cancelled() {
                break;
            }
            let line = line.map_err(|e| LlmError::Http(e.to_string()))?;
            match parse_sse_line(&line)? {
                SseLine::Chunk { delta, usage: chunk_usage, tool_calls } => {
                    if let Some(text) = delta {
                        on_delta(&text);
                        full.push_str(&text);
                    }
                    // Only the trailing usage-only chunk is expected to
                    // carry this, but take whichever chunk has it rather
                    // than assuming position.
                    if chunk_usage.is_some() {
                        usage = chunk_usage;
                    }
                    tool_calls_acc.ingest(tool_calls);
                }
                SseLine::Ignore => {}
                SseLine::Done => break,
            }
        }
        Ok(ChatStreamResult { text: full, usage, tool_calls: tool_calls_acc.finish() })
    }

    fn list_models(&self) -> Result<Vec<LlmModelInfo>, LlmError> {
        let mut response = self
            .agent
            .get(self.models_url())
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .call()
            .map_err(|e| LlmError::Http(e.to_string()))?;
        response = ok_or_status_error(response)?;

        let parsed: ModelsListResponse = response
            .body_mut()
            .read_json()
            .map_err(|e| LlmError::Http(e.to_string()))?;

        Ok(parsed.data.into_iter().map(|d| LlmModelInfo { id: d.id }).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(base_url: &str) -> OpenAiCompatibleProvider {
        OpenAiCompatibleProvider::new(
            build_agent(None).unwrap(),
            base_url.to_string(),
            "key".to_string(),
        )
    }

    #[test]
    fn chat_url_strips_trailing_slash() {
        let p = provider("https://api.example.com/v1/");
        assert_eq!(p.chat_url(), "https://api.example.com/v1/chat/completions");
    }

    #[test]
    fn models_url_strips_trailing_slash() {
        let p = provider("https://api.example.com/v1/");
        assert_eq!(p.models_url(), "https://api.example.com/v1/models");
    }

    #[test]
    fn request_omits_tools_and_tool_choice_when_no_tools_given() {
        let body = ChatCompletionRequest {
            model: "gpt-4o-mini",
            messages: vec![WireMessage {
                role: "user",
                content: Some("hi"),
                tool_call_id: None,
                tool_calls: vec![],
            }],
            tools: vec![],
            tool_choice: None,
            stream: false,
            stream_options: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert_eq!(
            json,
            r#"{"model":"gpt-4o-mini","messages":[{"role":"user","content":"hi"}]}"#
        );
    }

    #[test]
    fn request_includes_tools_and_tool_choice_when_tools_given() {
        let params = serde_json::json!({"type": "object", "properties": {}});
        let body = ChatCompletionRequest {
            model: "gpt-4o-mini",
            messages: vec![WireMessage {
                role: "user",
                content: Some("hi"),
                tool_call_id: None,
                tool_calls: vec![],
            }],
            tools: vec![WireTool {
                kind: "function",
                function: WireFunction { name: "read_file", description: "reads a file", parameters: &params },
            }],
            tool_choice: Some("auto"),
            stream: false,
            stream_options: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains(r#""tool_choice":"auto""#));
        assert!(json.contains(r#""type":"function""#));
        assert!(json.contains(r#""name":"read_file""#));
    }

    #[test]
    fn tool_message_serializes_null_content_with_tool_call_id() {
        let body =
            WireMessage { role: "tool", content: None, tool_call_id: Some("call_1"), tool_calls: vec![] };
        let json = serde_json::to_string(&body).unwrap();
        assert_eq!(json, r#"{"role":"tool","tool_call_id":"call_1"}"#);
    }

    #[test]
    fn assistant_message_serializes_tool_calls_when_present() {
        let body = WireMessage {
            role: "assistant",
            content: None,
            tool_call_id: None,
            tool_calls: vec![WireToolCallOut {
                id: "call_1",
                kind: "function",
                function: WireToolCallFunctionOut { name: "read_file", arguments: r#"{"path":"a.md"}"# },
            }],
        };
        let json = serde_json::to_string(&body).unwrap();
        assert_eq!(
            json,
            r#"{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a.md\"}"}}]}"#
        );
    }

    #[test]
    fn parses_a_plain_text_response() {
        let json = r#"{"choices":[{"message":{"role":"assistant","content":"hello"}}]}"#;
        let parsed: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.choices[0].message.content.as_deref(), Some("hello"));
        assert!(parsed.choices[0].message.tool_calls.is_empty());
    }

    #[test]
    fn parses_a_tool_calls_response_with_null_content_and_keeps_arguments_as_a_raw_string() {
        let json = r#"{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_abc123","type":"function","function":{"name":"get_current_weather","arguments":"{\n\"location\": \"Boston, MA\"\n}"}}]}}]}"#;
        let parsed: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        let message = &parsed.choices[0].message;
        assert_eq!(message.content, None);
        assert_eq!(message.tool_calls.len(), 1);
        assert_eq!(message.tool_calls[0].id, "call_abc123");
        assert_eq!(message.tool_calls[0].function.name, "get_current_weather");
        assert_eq!(message.tool_calls[0].function.arguments, "{\n\"location\": \"Boston, MA\"\n}");
    }

    #[test]
    fn parses_the_models_list_response_shape() {
        let json = r#"{"object":"list","data":[{"id":"model-a","object":"model","created":1,"owned_by":"org"},{"id":"model-b","object":"model","created":2,"owned_by":"org"}]}"#;
        let parsed: ModelsListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.data.len(), 2);
        assert_eq!(parsed.data[0].id, "model-a");
    }

    #[test]
    fn ok_or_status_error_passes_through_a_success_response() {
        let body = ureq::Body::builder().data("ok");
        let response = http::Response::builder().status(200).body(body).unwrap();
        let response = ok_or_status_error(response).unwrap();
        assert_eq!(response.status(), 200);
    }

    #[test]
    fn ok_or_status_error_includes_the_response_body_on_failure() {
        let body = ureq::Body::builder().data("{\"error\":\"invalid tool_calls format\"}");
        let response = http::Response::builder().status(500).body(body).unwrap();
        let err = ok_or_status_error(response).unwrap_err();
        assert!(matches!(
            err,
            LlmError::Http(msg)
                if msg.contains("500") && msg.contains("invalid tool_calls format")
        ));
    }

    #[test]
    fn ok_or_status_error_truncates_an_oversized_body() {
        let long_body = "x".repeat(ERROR_BODY_MAX_CHARS + 500);
        let body = ureq::Body::builder().data(long_body);
        let response = http::Response::builder().status(502).body(body).unwrap();
        let err = ok_or_status_error(response).unwrap_err();
        let LlmError::Http(msg) = err else { panic!("expected LlmError::Http") };
        assert!(msg.contains("(truncated)"));
        assert!(msg.len() < ERROR_BODY_MAX_CHARS + 100, "message should be capped, got {} chars", msg.len());
    }

    #[test]
    fn build_agent_succeeds_with_no_trust_cert_override() {
        assert!(build_agent(None).is_ok());
    }

    #[test]
    fn build_agent_rejects_a_malformed_pem() {
        let err = build_agent(Some("not a real pem")).unwrap_err();
        assert!(matches!(err, LlmError::Tls(_)));
    }

    #[test]
    fn build_body_sets_stream_true_for_streaming_requests() {
        let p = provider("https://api.example.com/v1");
        let request = ChatRequest { messages: vec![], tools: vec![], model: "m".to_string() };
        let json = serde_json::to_string(&p.build_body(&request, true)).unwrap();
        assert!(json.contains(r#""stream":true"#));
    }

    #[test]
    fn build_body_requests_usage_on_streaming_requests() {
        let p = provider("https://api.example.com/v1");
        let request = ChatRequest { messages: vec![], tools: vec![], model: "m".to_string() };
        let json = serde_json::to_string(&p.build_body(&request, true)).unwrap();
        assert!(json.contains(r#""stream_options":{"include_usage":true}"#));
    }

    #[test]
    fn build_body_omits_stream_for_non_streaming_requests() {
        let p = provider("https://api.example.com/v1");
        let request = ChatRequest { messages: vec![], tools: vec![], model: "m".to_string() };
        let json = serde_json::to_string(&p.build_body(&request, false)).unwrap();
        assert!(!json.contains("stream"));
    }

    #[test]
    fn parse_sse_line_extracts_delta_text() {
        let line = r#"data: {"choices":[{"delta":{"content":"Hel"}}]}"#;
        assert_eq!(
            parse_sse_line(line).unwrap(),
            SseLine::Chunk { delta: Some("Hel".to_string()), usage: None, tool_calls: vec![] }
        );
    }

    #[test]
    fn parse_sse_line_treats_missing_content_as_no_delta() {
        let line = r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#;
        assert_eq!(parse_sse_line(line).unwrap(), SseLine::Chunk { delta: None, usage: None, tool_calls: vec![] });
    }

    #[test]
    fn parse_sse_line_extracts_usage_from_the_trailing_chunk() {
        let line = r#"data: {"choices":[{"delta":{},"finish_reason":null}],"usage":{"prompt_tokens":67,"completion_tokens":2,"total_tokens":69}}"#;
        assert_eq!(
            parse_sse_line(line).unwrap(),
            SseLine::Chunk {
                delta: None,
                usage: Some(ChatUsage { prompt_tokens: 67, completion_tokens: 2, total_tokens: 69 }),
                tool_calls: vec![],
            }
        );
    }

    #[test]
    fn parse_sse_line_handles_a_chunk_with_neither_delta_nor_usage() {
        let line = r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#;
        assert_eq!(parse_sse_line(line).unwrap(), SseLine::Chunk { delta: None, usage: None, tool_calls: vec![] });
    }

    #[test]
    fn parse_sse_line_recognizes_the_done_sentinel() {
        assert_eq!(parse_sse_line("data: [DONE]").unwrap(), SseLine::Done);
    }

    #[test]
    fn parse_sse_line_ignores_blank_separator_lines() {
        assert_eq!(parse_sse_line("").unwrap(), SseLine::Ignore);
    }

    #[test]
    fn parse_sse_line_ignores_non_data_lines() {
        assert_eq!(parse_sse_line("event: ping").unwrap(), SseLine::Ignore);
    }

    #[test]
    fn parse_sse_line_errors_clearly_on_malformed_json() {
        let err = parse_sse_line("data: {not json}").unwrap_err();
        assert!(matches!(err, LlmError::Provider(_)));
    }

    #[test]
    fn parse_sse_line_strips_trailing_carriage_return() {
        let line = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\r";
        assert_eq!(
            parse_sse_line(line).unwrap(),
            SseLine::Chunk { delta: Some("hi".to_string()), usage: None, tool_calls: vec![] }
        );
    }

    #[test]
    fn parse_sse_line_extracts_a_single_chunk_tool_call_fragment() {
        let line = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a.md\"}"}}]}}]}"#;
        let parsed = parse_sse_line(line).unwrap();
        assert_eq!(
            parsed,
            SseLine::Chunk {
                delta: None,
                usage: None,
                tool_calls: vec![StreamToolCallDelta {
                    index: Some(0),
                    id: Some("call_1".to_string()),
                    function: Some(StreamToolCallFunctionDelta {
                        name: Some("read_file".to_string()),
                        arguments: Some(r#"{"path":"a.md"}"#.to_string()),
                    }),
                }]
            }
        );
    }

    #[test]
    fn parse_sse_line_treats_an_explicit_null_tool_calls_as_none() {
        // Real-world regression: once `tools` is present on the request,
        // several OpenAI-compatible servers explicitly send
        // `"tool_calls":null` on a content-only chunk instead of omitting
        // the key — plain `#[serde(default)]` only covers a missing key,
        // and `Vec<T>`'s own `Deserialize` otherwise rejects `null`
        // ("invalid type: null, expected a sequence").
        let line = r#"data: {"choices":[{"delta":{"content":"Hello","tool_calls":null}}]}"#;
        assert_eq!(
            parse_sse_line(line).unwrap(),
            SseLine::Chunk { delta: Some("Hello".to_string()), usage: None, tool_calls: vec![] }
        );
    }

    #[test]
    fn parses_a_response_with_an_explicit_null_tool_calls() {
        let json = r#"{"choices":[{"message":{"role":"assistant","content":"hi","tool_calls":null}}]}"#;
        let parsed: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        assert!(parsed.choices[0].message.tool_calls.is_empty());
    }

    #[test]
    fn tool_call_accumulator_merges_a_single_chunk_call() {
        let mut acc = ToolCallAccumulator::default();
        acc.ingest(vec![StreamToolCallDelta {
            index: Some(0),
            id: Some("call_1".to_string()),
            function: Some(StreamToolCallFunctionDelta {
                name: Some("readFile".to_string()),
                arguments: Some(r#"{"path":"a.md"}"#.to_string()),
            }),
        }]);
        let calls = acc.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "readFile");
        assert_eq!(calls[0].arguments, r#"{"path":"a.md"}"#);
    }

    #[test]
    fn tool_call_accumulator_merges_arguments_across_multiple_fragments_by_index() {
        let mut acc = ToolCallAccumulator::default();
        acc.ingest(vec![StreamToolCallDelta {
            index: Some(0),
            id: Some("call_1".to_string()),
            function: Some(StreamToolCallFunctionDelta {
                name: Some("readFile".to_string()),
                arguments: Some(r#"{"pa"#.to_string()),
            }),
        }]);
        acc.ingest(vec![StreamToolCallDelta {
            index: Some(0),
            id: None,
            function: Some(StreamToolCallFunctionDelta {
                name: None,
                arguments: Some(r#"th":"a"#.to_string()),
            }),
        }]);
        acc.ingest(vec![StreamToolCallDelta {
            index: Some(0),
            id: None,
            function: Some(StreamToolCallFunctionDelta {
                name: None,
                arguments: Some(r#".md"}"#.to_string()),
            }),
        }]);
        let calls = acc.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "readFile");
        assert_eq!(calls[0].arguments, r#"{"path":"a.md"}"#);
    }

    #[test]
    fn tool_call_accumulator_keeps_two_interleaved_indices_separate() {
        let mut acc = ToolCallAccumulator::default();
        acc.ingest(vec![
            StreamToolCallDelta {
                index: Some(0),
                id: Some("call_0".to_string()),
                function: Some(StreamToolCallFunctionDelta {
                    name: Some("listFiles".to_string()),
                    arguments: Some("{".to_string()),
                }),
            },
            StreamToolCallDelta {
                index: Some(1),
                id: Some("call_1".to_string()),
                function: Some(StreamToolCallFunctionDelta {
                    name: Some("readFile".to_string()),
                    arguments: Some(r#"{"path":"#.to_string()),
                }),
            },
        ]);
        acc.ingest(vec![
            StreamToolCallDelta { index: Some(0), id: None, function: Some(StreamToolCallFunctionDelta { name: None, arguments: Some("}".to_string()) }) },
            StreamToolCallDelta { index: Some(1), id: None, function: Some(StreamToolCallFunctionDelta { name: None, arguments: Some(r#""a.md"}"#.to_string()) }) },
        ]);
        let calls = acc.finish();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "call_0");
        assert_eq!(calls[0].name, "listFiles");
        assert_eq!(calls[0].arguments, "{}");
        assert_eq!(calls[1].id, "call_1");
        assert_eq!(calls[1].name, "readFile");
        assert_eq!(calls[1].arguments, r#"{"path":"a.md"}"#);
    }

    #[test]
    fn tool_call_accumulator_treats_a_missing_index_fragment_as_its_own_entry() {
        let mut acc = ToolCallAccumulator::default();
        acc.ingest(vec![
            StreamToolCallDelta {
                index: None,
                id: Some("call_a".to_string()),
                function: Some(StreamToolCallFunctionDelta {
                    name: Some("listFiles".to_string()),
                    arguments: Some("{}".to_string()),
                }),
            },
            StreamToolCallDelta {
                index: None,
                id: Some("call_b".to_string()),
                function: Some(StreamToolCallFunctionDelta {
                    name: Some("semanticSearch".to_string()),
                    arguments: Some(r#"{"query":"x"}"#.to_string()),
                }),
            },
        ]);
        let calls = acc.finish();
        assert_eq!(
            calls.len(),
            2,
            "two fragments both missing `index` must not be merged into one call"
        );
        assert_eq!(calls[0].id, "call_a");
        assert_eq!(calls[1].id, "call_b");
    }
}
