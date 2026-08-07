//! Types for the LLM chat client: provider configuration, the merged
//! provider view returned to the frontend, and the request/response shapes
//! a future chat feature will send/receive. Protocol-agnostic by design —
//! this module knows nothing about OpenAI's specific wire format
//! (`tool_calls`, `content: null`, etc.); that translation lives in
//! `infra::llm_providers::openai_compatible`, mirroring how
//! `domain::embeddings` stays agnostic while
//! `infra::embedding_providers::remote` carries the OpenAI-specific
//! `/embeddings` shape.
//!
//! Deliberately no `LlmProviderKind` enum: every provider (system or
//! custom) speaks the OpenAI-compatible protocol today, and
//! `infra::llm_providers::provider_for` is the one place that would grow a
//! branch if a genuinely different protocol (e.g. Anthropic-native) were
//! added later — the same reasoning `domain::embeddings::
//! EmbeddingProviderKind` followed: that enum only grew a second variant
//! once a second real implementation existed, not in anticipation of one.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One entry in a project-independent, globally-persisted list of
/// configured LLM providers (`AppSettings.llm.providers`). For a `System`
/// provider (an id that also appears in the compiled-in manifest, see
/// `infra::llm_provider_manifest`), this is an **override** — every field
/// is `Option` and `None` means "inherit from the manifest preset", not
/// "unset". For a fully custom (non-system) id, this is the whole
/// definition, typically built up field-by-field by an "add provider" form
/// before the user finishes it — hence every field being optional here
/// too, rather than requiring `base_url`/`model` up front.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmProviderConfig {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    /// An explicit pin, overriding both the manifest's `default_model` and
    /// the "ask the live `/models` endpoint, take the first result"
    /// fallback — see `services::llm_config::effective_model`.
    #[serde(default)]
    pub model: Option<String>,
    /// Overrides the manifest's baked-in `trusted_cert_pem` for a system
    /// provider (and takes priority over it — see
    /// `services::llm_config::resolve_provider`), or supplies one outright
    /// for a custom provider hitting a TLS endpoint the public root store
    /// doesn't already trust.
    #[serde(default)]
    pub trusted_cert_pem: Option<String>,
}

/// Persisted globally (`AppSettings.llm`) — provider configuration is not
/// per-project, same reasoning as `EmbeddingProviderConfig`. `providers` is
/// a `Vec`, not a `HashMap<String, _>`, so `settings.json` stays
/// human-diffable and mirrors the manifest's own array shape; provider
/// counts are always small enough that linear lookup by id is fine.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmSettings {
    #[serde(default)]
    pub active_provider_id: Option<String>,
    #[serde(default)]
    pub providers: Vec<LlmProviderConfig>,
}

/// One entry from the compiled-in provider manifest (see
/// `infra::llm_provider_manifest`) — what a downstream fork edits to bake
/// in its own provider(s), or empties out to ship with none, without
/// touching any `.rs` file. Deserialize-only: this is read from the
/// embedded JSON at startup and never written back out.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmProviderPreset {
    pub id: String,
    pub label: String,
    pub base_url: String,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub trusted_cert_pem: Option<String>,
}

/// The merged, ready-to-use view of one provider — a manifest preset (if
/// any) folded with its settings-layer override (if any), or a standalone
/// custom provider. What `services::llm_config::resolve_provider`/
/// `list_resolved_providers` produce, and what the frontend actually reads
/// to render the provider picker. Serialize-only: never deserialized back
/// from JSON, only ever computed.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLlmProvider {
    pub id: String,
    pub label: String,
    pub base_url: String,
    /// `true` if `id` matches a compiled-in manifest preset — drives the
    /// "встроенный"/"свой" badge and whether the frontend allows editing
    /// `base_url` or offers a delete action (system providers are only
    /// removable by editing the manifest and rebuilding, per
    /// `infra::llm_provider_manifest`'s doc comment).
    pub is_system: bool,
    /// `None` means no explicit pin exists yet (from either layer) — the
    /// caller should resolve it live via `effective_model` before actually
    /// building a `ChatRequest`, rather than treating `None` as a usable
    /// model name.
    pub model: Option<String>,
    pub trusted_cert_pem: Option<String>,
}

/// A chat message's speaker. `Tool` is needed even though nothing sends one
/// yet — feeding a tool's result back to the model as a message is how a
/// future multi-turn tool-calling loop works, and this type shouldn't need
/// a breaking change once that loop exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LlmRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmMessage {
    pub role: LlmRole,
    /// `None` for an `Assistant` message that only requests tool calls
    /// (matches the wire reality: OpenAI sends `content: null`, not `""`,
    /// in that case).
    #[serde(default)]
    pub content: Option<String>,
    /// `Some` only for a `Tool`-role message — which tool call this is the
    /// result of.
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

/// One callable tool, as the model needs to see it. `parameters` is a raw
/// JSON Schema object — deliberately `serde_json::Value` rather than a
/// bespoke schema type; this client doesn't need to validate or construct
/// schemas, only pass one through to the provider and back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub messages: Vec<LlmMessage>,
    #[serde(default)]
    pub tools: Vec<LlmToolDefinition>,
    /// Callers resolve the effective model (pin, manifest default, or
    /// first live result — see `services::llm_config::effective_model`)
    /// before building this; this type doesn't do that resolution itself.
    pub model: String,
}

/// One tool call the model is requesting. `arguments` is kept as the raw
/// JSON-encoded string the wire format actually carries (OpenAI: a JSON
/// object serialized *as a string*, not a nested object) — parsing it into
/// concrete tool arguments is `services::ai_tools`'s job later, once this
/// client is wired into the tool-execution loop, not this layer's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    /// `None` when the model only requested tool calls this turn.
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<LlmToolCall>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmModelInfo {
    pub id: String,
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("provider error: {0}")]
    Provider(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("tls configuration error: {0}")]
    Tls(String),
    #[error("{0}")]
    Message(String),
}

/// One LLM backend, selected by `infra::llm_providers::provider_for` from a
/// `ResolvedLlmProvider` + API key. Synchronous (not `async fn`), same
/// reasoning as `domain::embeddings::EmbeddingProvider`: this project's
/// `tokio` dependency only enables `sync, rt, macros, time` (no `net`), and
/// a request-per-call blocking HTTP client doesn't justify expanding that —
/// callers run this inside `spawn_blocking`.
pub trait LlmProvider: Send + Sync {
    fn chat(&self, request: ChatRequest) -> Result<ChatResponse, LlmError>;
    fn list_models(&self) -> Result<Vec<LlmModelInfo>, LlmError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_config_defaults_to_no_overrides() {
        let config: LlmProviderConfig = serde_json::from_str(r#"{"id":"custom-1"}"#).unwrap();
        assert_eq!(config.id, "custom-1");
        assert_eq!(config.label, None);
        assert_eq!(config.base_url, None);
        assert_eq!(config.model, None);
        assert_eq!(config.trusted_cert_pem, None);
    }

    #[test]
    fn settings_default_to_empty() {
        let settings = LlmSettings::default();
        assert_eq!(settings.active_provider_id, None);
        assert!(settings.providers.is_empty());
    }

    #[test]
    fn preset_deserializes_with_only_required_fields() {
        let preset: LlmProviderPreset = serde_json::from_str(
            r#"{"id":"alfagen","label":"AlfaGen","baseUrl":"https://example.internal"}"#,
        )
        .unwrap();
        assert_eq!(preset.default_model, None);
        assert_eq!(preset.trusted_cert_pem, None);
    }

    #[test]
    fn chat_message_omits_null_fields_are_still_readable() {
        // Mirrors the actual wire reality this type models: an
        // assistant message with tool calls has `content: null`.
        let message: LlmMessage =
            serde_json::from_str(r#"{"role":"assistant","content":null}"#).unwrap();
        assert_eq!(message.role, LlmRole::Assistant);
        assert_eq!(message.content, None);
        assert_eq!(message.tool_call_id, None);
    }

    #[test]
    fn tool_call_arguments_stay_a_raw_string() {
        let call: LlmToolCall = serde_json::from_str(
            r#"{"id":"call_1","name":"read_file","arguments":"{\"path\":\"a.md\"}"}"#,
        )
        .unwrap();
        assert_eq!(call.arguments, r#"{"path":"a.md"}"#);
    }
}
