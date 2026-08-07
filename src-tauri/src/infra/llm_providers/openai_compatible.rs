//! OpenAI-compatible chat-completions client — mirrors
//! `infra::embedding_providers::remote`'s shape closely, with one
//! deliberate deviation: a **per-provider `ureq::Agent`** rather than the
//! free functions (`ureq::post`/`ureq::get`) that sibling uses. TLS trust
//! configuration (a corporate internal CA, for a provider like AlfaGen) is
//! configured per-`Agent`, not global, so each provider gets its own agent
//! built once at construction time via `build_agent`.
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

/// Builds the `ureq::Agent` a provider's requests go through. When
/// `trusted_cert_pem` is `Some`, its certificates **replace** the agent's
/// trust store entirely (`RootCerts::Specific`, not additive to the public
/// WebPki roots) — correct here since a provider either needs its own CA
/// trusted (an internal endpoint like AlfaGen) or doesn't; providers
/// without an override keep the default `RootCerts::WebPki`, and since
/// every provider gets its own `Agent`, there's no cross-provider trust
/// interference either way.
pub fn build_agent(trusted_cert_pem: Option<&str>) -> Result<ureq::Agent, LlmError> {
    let mut builder = ureq::Agent::config_builder();
    if let Some(pem) = trusted_cert_pem {
        let certs = parse_trusted_certs(pem)?;
        let tls_config = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::Specific(std::sync::Arc::new(certs)))
            .build();
        builder = builder.tls_config(tls_config);
    }
    Ok(builder.build().new_agent())
}

/// Parses **every** certificate in `pem`, not just the first — unlike
/// `ureq::tls::Certificate::from_pem`, which is documented to pick only
/// the first certificate it finds. A corporate internal CA is commonly
/// issued as a chain (a root CA plus one or more intermediate CAs), and a
/// user pasting that whole chain as one concatenated PEM blob (or a
/// downstream fork baking it into the manifest) expects all of it trusted,
/// not silently just whichever certificate happens to appear first.
/// Errors if `pem` contains no certificate at all (mirrors
/// `Certificate::from_pem`'s "no pem encoded cert found" error for that
/// case).
fn parse_trusted_certs(pem: &str) -> Result<Vec<ureq::tls::Certificate<'static>>, LlmError> {
    let certs = ureq::tls::parse_pem(pem.as_bytes())
        .filter_map(|item| match item {
            Ok(ureq::tls::PemItem::Certificate(cert)) => Some(Ok(cert)),
            Err(e) => Some(Err(e)),
            // `PemItem` is `#[non_exhaustive]` — anything besides a
            // certificate (e.g. a private key, if one were pasted
            // alongside) isn't a trust root and is skipped rather than
            // erroring, same as `PrivateKey` items are today.
            Ok(_) => None,
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| LlmError::Tls(e.to_string()))?;
    if certs.is_empty() {
        return Err(LlmError::Tls("no PEM-encoded certificate found".to_string()));
    }
    Ok(certs)
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
    #[serde(default)]
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
    /// both, so these are independent, not an either/or.
    Chunk { delta: Option<String>, usage: Option<ChatUsage> },
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
    let delta = chunk.choices.into_iter().next().and_then(|c| c.delta.content);
    let usage = chunk.usage.map(ChatUsage::from);
    Ok(SseLine::Chunk { delta, usage })
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
    ) -> Result<ChatStreamResult, LlmError> {
        let body = self.build_body(&request, true);

        let response = self
            .agent
            .post(self.chat_url())
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .send_json(&body)
            .map_err(|e| LlmError::Http(e.to_string()))?;

        let reader = std::io::BufReader::new(response.into_body().into_reader());
        let mut full = String::new();
        let mut usage = None;
        for line in std::io::BufRead::lines(reader) {
            let line = line.map_err(|e| LlmError::Http(e.to_string()))?;
            match parse_sse_line(&line)? {
                SseLine::Chunk { delta, usage: chunk_usage } => {
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
                }
                SseLine::Ignore => {}
                SseLine::Done => break,
            }
        }
        Ok(ChatStreamResult { text: full, usage })
    }

    fn list_models(&self) -> Result<Vec<LlmModelInfo>, LlmError> {
        let mut response = self
            .agent
            .get(self.models_url())
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .call()
            .map_err(|e| LlmError::Http(e.to_string()))?;

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
            messages: vec![WireMessage { role: "user", content: Some("hi"), tool_call_id: None }],
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
            messages: vec![WireMessage { role: "user", content: Some("hi"), tool_call_id: None }],
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
        let body = WireMessage { role: "tool", content: None, tool_call_id: Some("call_1") };
        let json = serde_json::to_string(&body).unwrap();
        assert_eq!(json, r#"{"role":"tool","tool_call_id":"call_1"}"#);
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
    fn build_agent_succeeds_with_no_trust_cert_override() {
        assert!(build_agent(None).is_ok());
    }

    #[test]
    fn build_agent_rejects_a_malformed_pem() {
        let err = build_agent(Some("not a real pem")).unwrap_err();
        assert!(matches!(err, LlmError::Tls(_)));
    }

    // Two throwaway self-signed certificates (`openssl req -x509 -newkey
    // rsa:2048 -nodes -days 1 -subj "/CN=test-root-N"`) — real, structurally
    // valid X.509/DER once base64-decoded, but not issued by anything and
    // not used to actually connect anywhere. Exist purely so
    // `parse_trusted_certs`/`build_agent` are tested against real PEM
    // encoding rather than hand-typed placeholder text.
    const TEST_CERT_1: &str = "-----BEGIN CERTIFICATE-----\n\
MIIDDTCCAfWgAwIBAgIUGpUPEU6cXRcVo6oEAizKXckdihcwDQYJKoZIhvcNAQEL\n\
BQAwFjEUMBIGA1UEAwwLdGVzdC1yb290LTEwHhcNMjYwODA3MTAxMDM5WhcNMjYw\n\
ODA4MTAxMDM5WjAWMRQwEgYDVQQDDAt0ZXN0LXJvb3QtMTCCASIwDQYJKoZIhvcN\n\
AQEBBQADggEPADCCAQoCggEBAKzBAwpDZvuKq3/aQJNh2EpezEGhcHY8mlo+6qHx\n\
B3Mp8ClBbUaFif7IxOM0xfSBrP8RjmzUFxg1n80456fwLNgkdRSopK5Gef6hQT1c\n\
6n2e2qIXPjgwoLQplAByAsUoojy0fT87HdFRNl7trjqDf1M8+l2aZt6hV7KWBwNK\n\
RiLwlAXhoWRhzk0lIeu12DFwEaYYYoU2GAObo9upUsnl3FZjTOMN614G9fHXi72J\n\
WTBCiTbKL2p4yd7olGjlSYAWx6Sjp4RTUO2mLYuuq5RNznuc0Q40j/DOMH+xYWw/\n\
LPqf5onSSm7wrBPocmkb5is+Dho0989VrcT83OBw27ZqKVECAwEAAaNTMFEwHQYD\n\
VR0OBBYEFPvdNpMkJZ//V2KiSSYDoIH2aGpgMB8GA1UdIwQYMBaAFPvdNpMkJZ//\n\
V2KiSSYDoIH2aGpgMA8GA1UdEwEB/wQFMAMBAf8wDQYJKoZIhvcNAQELBQADggEB\n\
AJ7hnJfnq2zt4sujG2GJ+imBMWI2H+NZiHhtYbu77S8/UC6OTc/7rQdFooeh2kTX\n\
h+KqHNoIxzubZg1TpzENjo2msJ8EhLhbHGAjMt1AsFxtfiepqAHfhQZvb4Pj+fJn\n\
hIKv3mq6TJh7i683UYrMno+RlXpPxqcIT+dpPTeVjTknofhEhg78sv9AhfxeCYS+\n\
o2luAw64b0TYXF8sf4Mx5IoOsfN20Hm+pj3nKQH/SpOLZwlgXQURWJwytxibck4W\n\
ruzJHn650tODkZyjFHnk62Cd4QKa8Jm86El9v80aIq25DWq/UCJKzYkbKLuktbwZ\n\
qeR/KGN8Bh7XSk/B/N/8gJQ=\n\
-----END CERTIFICATE-----\n";

    const TEST_CERT_2: &str = "-----BEGIN CERTIFICATE-----\n\
MIIDDTCCAfWgAwIBAgIUXKODgC8Vp4Zo6zpfD6dXtJz7W8gwDQYJKoZIhvcNAQEL\n\
BQAwFjEUMBIGA1UEAwwLdGVzdC1yb290LTIwHhcNMjYwODA3MTAxMDM5WhcNMjYw\n\
ODA4MTAxMDM5WjAWMRQwEgYDVQQDDAt0ZXN0LXJvb3QtMjCCASIwDQYJKoZIhvcN\n\
AQEBBQADggEPADCCAQoCggEBANX8US+mSoyHjk5ul8cTffGNWStsaRjJg+b8tXMj\n\
w6p3yl9laSdHZn5507zePh/madO5cxWQFWxAzO21HPNHaPYX9rnNVurHNDdeYIx/\n\
4arjjJESJP/D84t41gkOqmd5oUPTdVJEO8uGGWRTGrsN+s6jB/TGpjxk83guyycP\n\
vKVNhtWUXBeX3agie7KaFxWnMgMC2Cq5Rn9BGEdPgTbWs8VUlo54IHZNDAggl0MB\n\
Kie+Y6vLQg67IRadNci0DMr7oG2sJkpYHC5YIpNI7+3nOv/tA8gOoMvna3a/vcMJ\n\
9GbqfwUywikvbj2sfavBj6Oz/rrzWHgKTb2lwGWq+3JrqhkCAwEAAaNTMFEwHQYD\n\
VR0OBBYEFAV8iu42XG8hACTT8sJwMNhBLWWQMB8GA1UdIwQYMBaAFAV8iu42XG8h\n\
ACTT8sJwMNhBLWWQMA8GA1UdEwEB/wQFMAMBAf8wDQYJKoZIhvcNAQELBQADggEB\n\
ACa7NdZaTqb3sBhv5yJ+VXu2KBkOaKuRtX1GpJwDA5NEkrXxGc5plZ6x322yaj4L\n\
RvRAPuTKG0yggPfeAjsUc9F93azXLYSdoGd8Nluuob7IbLaKjyVXMMr33cfY1Rkn\n\
XisLoCs5m2KSB96izGn/F2JDbTFbWtHDAQgrCO7gvOQ3sQfzaRhiHiRnuX8c+18A\n\
EdPIXI8UYm6De+fhi7iXIFRjHoYWcm15gr/A2hpb2/f6fBdcsn5l9F15iScu6tF3\n\
92SNkKhKb3b+I32HzRrFoLoL/QQHUOC0SGhXVbcyQ8FdYs45/WFkuFUvS2NkqT+C\n\
YoQlQIWF38mOPRxLRBxKA7g=\n\
-----END CERTIFICATE-----\n";

    #[test]
    fn parse_trusted_certs_extracts_a_single_certificate() {
        let certs = parse_trusted_certs(TEST_CERT_1).unwrap();
        assert_eq!(certs.len(), 1);
    }

    #[test]
    fn parse_trusted_certs_extracts_every_certificate_in_a_concatenated_chain() {
        let chain = format!("{TEST_CERT_1}{TEST_CERT_2}");
        let certs = parse_trusted_certs(&chain).unwrap();
        assert_eq!(certs.len(), 2, "both certificates in the chain must be trusted, not just the first");
    }

    #[test]
    fn build_agent_succeeds_with_a_multi_certificate_chain() {
        let chain = format!("{TEST_CERT_1}{TEST_CERT_2}");
        assert!(build_agent(Some(&chain)).is_ok());
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
            SseLine::Chunk { delta: Some("Hel".to_string()), usage: None }
        );
    }

    #[test]
    fn parse_sse_line_treats_missing_content_as_no_delta() {
        let line = r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#;
        assert_eq!(parse_sse_line(line).unwrap(), SseLine::Chunk { delta: None, usage: None });
    }

    #[test]
    fn parse_sse_line_extracts_usage_from_the_trailing_chunk() {
        let line = r#"data: {"choices":[{"delta":{},"finish_reason":null}],"usage":{"prompt_tokens":67,"completion_tokens":2,"total_tokens":69}}"#;
        assert_eq!(
            parse_sse_line(line).unwrap(),
            SseLine::Chunk {
                delta: None,
                usage: Some(ChatUsage { prompt_tokens: 67, completion_tokens: 2, total_tokens: 69 }),
            }
        );
    }

    #[test]
    fn parse_sse_line_handles_a_chunk_with_neither_delta_nor_usage() {
        let line = r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#;
        assert_eq!(parse_sse_line(line).unwrap(), SseLine::Chunk { delta: None, usage: None });
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
            SseLine::Chunk { delta: Some("hi".to_string()), usage: None }
        );
    }
}
