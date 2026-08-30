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

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::ai_tools::{Task, ToolResult};

/// Substituted with a fresh UUID on each HTTP request when used as a
/// header value in `request_headers` (manifest preset or settings override).
pub const REQUEST_HEADER_VALUE_UUID: &str = "$uuid";

/// Token limits for an LLM **provider** (endpoint), not for an individual
/// model id — informational only, not enforced in this client; shown in the
/// chat UI context bar. Independent of which model is pinned or auto-selected.
/// Not fetched live from `/models`; comes from the manifest preset and/or a
/// settings-layer provider override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTokenLimit {
    pub context: u32,
    pub output: u32,
}

/// Fallback when a custom provider has no explicit `limit` in settings.
/// Drives the chat context ring and compaction trigger for non-system ids.
pub const DEFAULT_PROVIDER_TOKEN_LIMIT: ProviderTokenLimit = ProviderTokenLimit {
    context: 200_000,
    output: 30_000,
};

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
    /// the auto-pin from the first live `/models` fetch — see
    /// `services::llm_config::effective_model`. Set to `None` («Авто» in
    /// Settings) to clear the pin and re-discover on next use.
    #[serde(default)]
    pub model: Option<String>,
    /// User-curated model ids for the chat/settings pickers — populated
    /// when the Settings tab refreshes the live `/models` list or saves a
    /// manual slug (OpenRouter-style providers). The chat panel reads this
    /// catalog instead of calling the provider API on every open.
    #[serde(default)]
    pub known_models: Vec<String>,
    /// Overrides the manifest's baked-in `trusted_cert_pem` for a system
    /// provider (and takes priority over it — see
    /// `services::llm_config::resolve_provider`), or supplies one outright
    /// for a custom provider hitting a TLS endpoint the public root store
    /// doesn't already trust.
    #[serde(default)]
    pub trusted_cert_pem: Option<String>,
    /// Overrides the manifest's baked-in `limit` for a system provider, or
    /// supplies one outright for a custom provider — same "override wins
    /// when `Some`" merge as the other fields, see
    /// `services::llm_config::resolve_provider`.
    #[serde(default)]
    pub limit: Option<ProviderTokenLimit>,
    /// Overrides the manifest's baked-in `request_headers` for a system
    /// provider, or supplies headers outright for a custom provider. When
    /// `Some`, replaces the preset map entirely — same merge as other
    /// override fields; `None` means inherit from the manifest.
    #[serde(default)]
    pub request_headers: Option<HashMap<String, String>>,
}

/// Persisted globally (`AppSettings.llm`) — provider configuration is not
/// per-project, same reasoning as `EmbeddingProviderConfig`. `providers` is
/// a `Vec`, not a `HashMap<String, _>`, so `settings.json` stays
/// human-diffable and mirrors the manifest's own array shape; provider
/// counts are always small enough that linear lookup by id is fine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmSettings {
    #[serde(default)]
    pub active_provider_id: Option<String>,
    #[serde(default)]
    pub providers: Vec<LlmProviderConfig>,
    /// Off by default — a conversation can contain sensitive document
    /// content the user may not want written to disk. When on,
    /// `commands::llm::llm_chat_stream` appends every request/response of
    /// every tool-calling round to `~/.atlas/logs/llm.jsonl` (see
    /// `infra::llm_debug_log`), so a provider error can be correlated with
    /// the exact payload that produced it.
    #[serde(default)]
    pub debug_logging: bool,
    /// When `true`, hides the follow-up suggestion chips the frontend shows
    /// above the chat transcript after the user picks a branching
    /// suggestion (see `AssistantConversation`'s `activeSuggestion` state).
    /// Never affects the initial suggestion chips shown in a brand-new,
    /// empty conversation — those always render regardless of this flag.
    /// Named so the derived `Default` (`false`) matches the desired
    /// "follow-ups on by default" behavior, same shape as `debug_logging`.
    #[serde(default)]
    pub follow_up_suggestions_disabled: bool,
    /// On by default — unlike `debug_logging`, this never stores raw
    /// document content (see `infra::tool_call_log::redact_args`/
    /// `redact_result`), only structural/identifying fields (tool name,
    /// path-shaped args, status, timing), so it's safe to leave on as an
    /// always-available audit trail. When on, `services::ai_tools::
    /// execute_tool_logged` (called from both the chat tool-calling loop
    /// and the standalone `ai_execute_tool` command) writes one redacted
    /// row per tool call to `~/.atlas/tool_calls.db` (see
    /// `infra::tool_call_log`), browsable from Инструменты → Журнал
    /// вызовов инструментов.
    #[serde(default = "default_true")]
    pub tool_call_logging: bool,
    /// On by default. When on, the frontend plays a short chime after an
    /// assistant turn finishes successfully (`ChatStreamOutcome::Done`)
    /// and, if the main window is unfocused, also sends an OS notification.
    /// Cancelled / errored turns stay silent. Frontend-only; the backend
    /// never reads this flag.
    #[serde(default = "default_true")]
    pub task_done_sound_enabled: bool,
    /// On by default. When on, the frontend plays a short chime when an
    /// `askUser` clarifying-question card appears mid-turn and, if the
    /// main window is unfocused, also sends an OS notification. Ordinary
    /// tool-approval cards stay silent. Frontend-only; the backend never
    /// reads this flag.
    #[serde(default = "default_true")]
    pub need_answer_sound_enabled: bool,
    /// On by default. When off, the status-bar rate-limit chip is hidden
    /// and completion tokens are not recorded — the baked-in rule from
    /// `system_providers.yaml` `rateLimits` stays unused until this is
    /// turned back on.
    #[serde(default = "default_true")]
    pub rate_limit_enabled: bool,
    /// On by default. When on, a tool-free extractor LLM call runs after
    /// each persisted chat turn and (subject to `memory_policy`) appends
    /// lasting facts to OptMem. Off skips the job entirely — wake of
    /// already-stored memory still injects on the next turn.
    #[serde(default = "default_true")]
    pub memory_extraction_enabled: bool,
    /// Minimum extractor `confidence` a candidate fact needs before policy
    /// will store it. Clamped to 0.0–1.0 at the policy boundary.
    #[serde(default = "default_memory_confidence")]
    pub memory_confidence_threshold: f32,
}

fn default_true() -> bool {
    true
}

fn default_memory_confidence() -> f32 {
    crate::domain::memory_policy::DEFAULT_CONFIDENCE_THRESHOLD
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            active_provider_id: None,
            providers: Vec::new(),
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
            rate_limit_enabled: true,
            memory_extraction_enabled: true,
            memory_confidence_threshold: crate::domain::memory_policy::DEFAULT_CONFIDENCE_THRESHOLD,
        }
    }
}

/// One entry from the compiled-in provider manifest (see
/// `infra::llm_provider_manifest`) — what a downstream fork edits to bake
/// in its own provider(s), or empties out to ship with none, without
/// touching any `.rs` file. Deserialize-only: this is read from the
/// embedded YAML at startup and never written back out.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LlmProviderPreset {
    pub id: String,
    pub label: String,
    pub base_url: String,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub trusted_cert_pem: Option<String>,
    #[serde(default)]
    pub limit: Option<ProviderTokenLimit>,
    /// Optional HTTP headers sent on every LLM request (`/chat/completions`,
    /// `/models`). Values of `$uuid` (see `REQUEST_HEADER_VALUE_UUID`) are
    /// replaced with a fresh UUID per request.
    #[serde(default)]
    pub request_headers: Option<HashMap<String, String>>,
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
    /// Saved model ids for the picker UI — see `LlmProviderConfig::known_models`.
    pub known_models: Vec<String>,
    /// Provider-level token limits (see `ProviderTokenLimit`) — unchanged
    /// when the pinned model changes.
    pub limit: Option<ProviderTokenLimit>,
    /// HTTP headers merged from manifest preset and settings override.
    pub request_headers: HashMap<String, String>,
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
    /// Non-empty only for an `Assistant` message that requested tool
    /// calls — round-tripped back to the provider so it sees its own prior
    /// request when the matching `Tool` result messages follow. `skip_serializing_if`
    /// keeps a plain `{role, content, toolCallId}` message (everything the
    /// frontend itself ever builds) byte-identical to before this field
    /// existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<LlmToolCall>,
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

/// Replaces `arguments` with `"{}"` on any call whose `arguments` doesn't
/// parse as a JSON *object* — used by `commands::llm::llm_chat_stream`
/// before round-tripping a model's tool calls back into the next request's
/// message history (never on the copy handed to `services::ai_tools::
/// parse_tool_call` for actual execution, which needs the real string to
/// report an accurate error back to the model).
///
/// A model occasionally streams malformed `arguments` (e.g. trailing
/// garbage after a complete JSON value — a real observed case:
/// `"{}\"\""`). The OpenAI wire format doesn't require the *echoed* copy to
/// be valid JSON (the field is opaque to the protocol), but at least one
/// real gateway 500s server-side when it is not — this sanitizes
/// defensively regardless of which provider is in use, since "malformed
/// JSON crashes a provider's own request validation" isn't something this
/// client can rely on any given provider tolerating.
pub fn sanitize_tool_call_arguments(calls: &[LlmToolCall]) -> Vec<LlmToolCall> {
    calls
        .iter()
        .map(|call| {
            let is_valid_object = serde_json::from_str::<serde_json::Value>(&call.arguments)
                .is_ok_and(|v| v.is_object());
            LlmToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: if is_valid_object { call.arguments.clone() } else { "{}".to_string() },
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    /// `None` when the model only requested tool calls this turn.
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<LlmToolCall>,
    /// Real token usage when the provider reported it on the final chunk
    /// (one-shot `chat` is implemented via streaming, so this is the same
    /// trailing `usage` field `chat_stream` already surfaces). `None` when
    /// the server omitted it — callers must not invent a count.
    #[serde(default)]
    pub usage: Option<ChatUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmModelInfo {
    pub id: String,
}

/// Token accounting for one completed chat turn, as an OpenAI-compatible
/// server reports it — protocol-agnostic here (see this module's top-level
/// doc comment); `infra::llm_providers::openai_compatible` carries the
/// wire-shaped equivalent and converts into this. Since every request
/// resends the full message history, `total_tokens` for the latest turn is
/// the authoritative context-window usage at that point in the
/// conversation, not just a per-turn stat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// `LlmProvider::chat_stream`'s return value: the authoritative full reply
/// text (a safety net against a dropped delta event), plus real usage if the
/// server reported one on the final chunk — `None` for a provider that
/// doesn't send it (the frontend falls back to its own character estimate
/// in that case, see `estimateTokenCount`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStreamResult {
    pub text: String,
    /// The reasoning-capable model's accumulated "thinking" text
    /// (`reasoning_content` on the wire), same authoritative-final-value
    /// role as `text` itself — a safety net against a dropped
    /// `on_reasoning` event on the way to a frontend. Empty for every
    /// provider/model that never sends `reasoning_content` at all.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reasoning: String,
    #[serde(default)]
    pub usage: Option<ChatUsage>,
    /// Non-empty when this round ended with the model requesting tool
    /// calls instead of (or alongside) a final answer. This is both
    /// `LlmProvider::chat_stream`'s per-round return value — the
    /// tool-calling loop in `commands::llm::llm_chat_stream` reads this to
    /// decide whether to continue — and, incidentally, the same type that
    /// command itself returns to the frontend; by construction the command
    /// only ever returns once a round's `tool_calls` is empty, so the
    /// frontend never actually observes a non-empty value here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<LlmToolCall>,
}

/// The `Done` payload: the same round result the frontend has always
/// received (`ChatStreamResult`, flattened — `text`/`usage`/`toolCalls`
/// land at the same top-level keys as before this type existed), plus the
/// turn's final `todos` state. A separate wrapper (not a field bolted onto
/// `ChatStreamResult` itself) so the provider layer
/// (`infra::llm_providers::openai_compatible`) and `infra::llm_debug_log`,
/// which both construct/consume `ChatStreamResult` with no notion of
/// todos, are untouched by this change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDone {
    #[serde(flatten)]
    pub result: ChatStreamResult,
    pub todos: Vec<Task>,
}

/// What one call to `commands::llm::llm_chat_stream`/`llm_chat_stream_resume`
/// resolves with: either a final answer, a round that hit at least one tool
/// call `domain::ai_access::call_requires_confirmation` flags
/// (nothing in that round has executed yet, and the caller must resolve
/// `PendingApproval` via `llm_chat_stream_resume` before the conversation
/// can continue), or the turn being stopped mid-flight by the user (see
/// `commands::llm::llm_cancel_chat`) — same `ChatDone` shape as `Done`
/// (`result.text` is whatever text had streamed in the round cancellation
/// landed in, `""` if it landed between rounds/tool calls instead), so the
/// frontend can reuse the same "correct the trailing text block, stop
/// showing the message as streaming" handling either way and only needs to
/// special-case the status tag for display (e.g. an "Остановлено" label
/// instead of nothing). Unlike `Done`, no tool call from the round that was
/// in flight when cancellation landed ever executes — see `run_tool_loop`'s
/// doc comment for exactly where this is checked.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", content = "value", rename_all = "camelCase")]
pub enum ChatStreamOutcome {
    Done(ChatDone),
    PendingApproval(PendingApproval),
    Cancelled(ChatDone),
}

/// A whole round paused, unexecuted. `history`/`round`/`budget_used` must
/// be sent back verbatim to `llm_chat_stream_resume` (along with the
/// caller's decisions) — the backend keeps no server-side session state,
/// so this is the entire resumable checkpoint.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApproval {
    /// History up to and including this round's `Assistant` tool-calls
    /// message. Nothing from this round has executed.
    pub history: Vec<LlmMessage>,
    /// Rounds consumed so far (this round included) — threaded through so
    /// resuming can't bypass `MAX_TOOL_ITERATIONS` by pausing repeatedly.
    pub round: u32,
    /// Weighted tool-call budget consumed so far (this round included,
    /// via `services::llm_chat::round_cost`) — same anti-bypass purpose as
    /// `round`, but sensitive to which tools were actually called instead
    /// of treating every round as equal cost. See `MAX_TOOL_BUDGET`.
    pub budget_used: u32,
    /// Every call this round requested, in original order — including
    /// non-risky calls bundled into the same round (their
    /// `requires_confirmation` is `false`; they need no decision and
    /// execute automatically on resume).
    pub calls: Vec<PendingToolCall>,
    /// The turn's `todos` state as of this pause. A round pausing means
    /// nothing in *that* round executed — but an earlier, already-completed
    /// round of the same multi-round turn may have already called `todo`,
    /// so this must carry the loop's accumulated state, not just "unchanged
    /// since turn start". Sent back unmodified to `llm_chat_stream_resume`,
    /// same as `history`.
    pub todos: Vec<Task>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub requires_confirmation: bool,
}

/// One decision on one pending call, supplied by the caller of
/// `llm_chat_stream_resume` — required for every `PendingToolCall` whose
/// `requires_confirmation` was `true` (validated server-side).
///
/// For `askUser`, `approved: true` plus `answer` carries the structured
/// responses; `approved: false` (or missing `answer`) means the user
/// skipped / stopped.
///
/// For `requestArtifact`, `approved: true` plus `artifact_id` names the
/// artifact the user filled in; the backend loads it from the store rather
/// than accepting its contents here, so what the model reads is provably
/// what was saved. `approved: false` (or a missing id) is «Заполню позже».
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallDecision {
    pub id: String,
    pub approved: bool,
    #[serde(default)]
    pub answer: Option<crate::domain::ai_tools::AskUserAnswerPayload>,
    #[serde(default)]
    pub artifact_id: Option<String>,
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
    /// Streams a completion, invoking `on_delta` once per non-empty text
    /// chunk as it arrives, and returning the full accumulated text once
    /// the stream ends — a caller that also wants the authoritative final
    /// text (e.g. as a safety net against a dropped event on the way to a
    /// frontend) reads the return value rather than re-deriving it from the
    /// callback calls. `&dyn Fn` (not a generic parameter) because this
    /// trait is used as a trait object (`Arc<dyn LlmProvider>`).
    ///
    /// `on_reasoning` is the same kind of callback for a reasoning-capable
    /// model's "thinking" text (`reasoning_content` on the wire) — fired
    /// separately from `on_delta` since a chunk carries one or the other,
    /// never both meaningfully at once. Most providers never call this at
    /// all, which is fine: it's simply never invoked.
    ///
    /// `on_tool_call_delta` is the same kind of callback for a streamed
    /// tool call: `(id, name, arguments)` after each SSE chunk that grew a
    /// call which already has an `id`. Arguments are the accumulation so
    /// far, not the fragment — a long `visualize`/`writeFile` payload
    /// otherwise sits invisible until the stream ends, which looks like a
    /// hung connection. Callers that only want the finished `tool_calls`
    /// (the `chat()` wrapper, scripted tests) pass a no-op.
    ///
    /// `cancelled` is polled between SSE chunks (see the implementation's
    /// own doc comment for exactly where) so a user-initiated stop
    /// (`commands::llm::llm_cancel_chat`) takes effect within roughly one
    /// chunk of being requested rather than only once the whole response
    /// has finished streaming — the underlying blocking socket read itself
    /// still can't be interrupted mid-chunk without a bigger client rework,
    /// so a stalled connection (no chunk arriving at all) isn't helped by
    /// this. Returning early this way is not an error: whatever text/tool
    /// calls had accumulated so far come back as a normal `Ok(..)`, same
    /// shape as a stream that ended naturally — the caller
    /// (`services::llm_chat::run_tool_loop`) is what decides, by checking the
    /// same flag itself right after this returns, whether to treat that as
    /// `Done` or `Cancelled`.
    fn chat_stream(
        &self,
        request: ChatRequest,
        on_delta: &dyn Fn(&str),
        on_reasoning: &dyn Fn(&str),
        on_tool_call_delta: &dyn Fn(&str, &str, &str),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ChatStreamResult, LlmError>;
    fn list_models(&self) -> Result<Vec<LlmModelInfo>, LlmError>;
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStreamDelta {
    pub delta: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStreamReasoning {
    pub delta: String,
}

pub const STEERING_PREFIX: &str =
    "[Уточнение от пользователя, не новое задание — учти в текущей работе]: ";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteeringAppliedEvent {
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallEvent {
    /// The model's own `LlmToolCall::id` — lets the frontend correlate this
    /// call with its later `ToolResultEvent` regardless of how many
    /// other calls/rounds happen in between.
    pub id: String,
    pub name: String,
    /// Raw JSON-encoded string, same as `LlmToolCall::arguments` — the
    /// frontend parses it if it wants structured display, this event
    /// doesn't pre-parse it.
    pub arguments: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultEvent {
    /// Matches the `id` on the `ToolCallEvent` this settles.
    pub id: String,
    /// `Some` on success — the same typed `ToolResult` that gets
    /// JSON-serialized into the wire `content` sent back to the model, just
    /// cloned rather than reserialized into a display string here.
    /// Formatting it into a human-readable summary is the frontend's job
    /// (`describeToolResult` in `src/lib/assistantConfig.ts`), matching how
    /// `describeToolActivity` already handles the "what is being done" line
    /// client-side rather than this command inventing display text.
    pub result: Option<ToolResult>,
    /// `Some` on failure — `ToolError`'s `Display` text (the same string
    /// that already goes into the `Tool` message's `content` as
    /// `"Error: {e}"`).
    pub error: Option<String>,
}

/// Everything a chat turn reports outward while it runs. `services::llm_chat`
/// hands these to a sink; `commands::llm` is the only thing that knows they
/// become Tauri events. Grouped into one enum rather than five separate
/// callbacks so adding a sixth kind of report does not change every
/// signature along the way.
#[derive(Debug, Clone)]
pub enum ChatEvent {
    Delta(ChatStreamDelta),
    Reasoning(ChatStreamReasoning),
    SteeringApplied(SteeringAppliedEvent),
    /// Fired while a tool call's `arguments` are still arriving on the
    /// SSE stream — same payload shape as `ToolCall`, but the JSON may be
    /// incomplete. Always followed later by `ToolCall` (execution starting)
    /// with the same `id`, unless the turn is cancelled first.
    ToolCallDelta(ToolCallEvent),
    /// Fired just before a tool executes; always followed by exactly one
    /// `ToolResult` carrying the same `id`. The frontend pairs them by that
    /// id (see `chatBlocks.ts`), so the order is a contract, not an
    /// incidental detail.
    ToolCall(ToolCallEvent),
    ToolResult(ToolResultEvent),
    /// The provider reported token usage and it has been recorded — the
    /// status-bar chip should re-read its snapshot.
    RateLimitChanged,
    /// The same usage report, for the chat panel's context ring rather than
    /// the rate-limit chip. Since every request resends the whole history,
    /// `total_tokens` here is the authoritative context size as of the round
    /// that just finished — the frontend's own character estimate only has
    /// to cover the tool results appended *after* it, until the next round
    /// reports again. Only `run_tool_loop` fires this: `llm_chat_once`'s
    /// usage belongs to a separate one-shot conversation (history
    /// compaction, the memory pipeline), not to the chat's context.
    ContextUsage(ChatUsage),
}

/// Where a turn's `ChatEvent`s go. A port like `LlmProvider`: the services
/// that report through it never learn what is on the other side, and
/// `commands::chat_events` is the only implementation that turns them into
/// Tauri events. `Arc<dyn Fn>` rather than a generic bound because the sink
/// is moved into the `on_delta` / `on_reasoning` / `on_tool_call_delta`
/// closures handed to `LlmProvider::chat_stream`, which outlive the call
/// that installed them.
pub type ChatEventSink = Arc<dyn Fn(ChatEvent) + Send + Sync>;

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
        assert!(settings.rate_limit_enabled);
        assert!(settings.memory_extraction_enabled);
        assert_eq!(
            settings.memory_confidence_threshold,
            crate::domain::memory_policy::DEFAULT_CONFIDENCE_THRESHOLD
        );
    }

    #[test]
    fn settings_missing_rate_limit_flag_defaults_on() {
        let settings: LlmSettings = serde_json::from_str("{}").unwrap();
        assert!(settings.rate_limit_enabled);
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
        assert_eq!(message.tool_calls, vec![]);
    }

    #[test]
    fn tool_call_arguments_stay_a_raw_string() {
        let call: LlmToolCall = serde_json::from_str(
            r#"{"id":"call_1","name":"read_file","arguments":"{\"path\":\"a.md\"}"}"#,
        )
        .unwrap();
        assert_eq!(call.arguments, r#"{"path":"a.md"}"#);
    }

    #[test]
    fn sanitize_tool_call_arguments_leaves_a_valid_object_untouched() {
        let calls = vec![LlmToolCall {
            id: "call_1".to_string(),
            name: "readFile".to_string(),
            arguments: r#"{"path":"a.md"}"#.to_string(),
        }];
        let sanitized = sanitize_tool_call_arguments(&calls);
        assert_eq!(sanitized, calls);
    }

    #[test]
    fn sanitize_tool_call_arguments_replaces_malformed_json_with_an_empty_object() {
        // The real observed case: valid `{}` followed by stray trailing
        // characters — `serde_json` rejects this as "trailing characters".
        let calls = vec![LlmToolCall {
            id: "call_1".to_string(),
            name: "listFiles".to_string(),
            arguments: "{}\"\"".to_string(),
        }];
        let sanitized = sanitize_tool_call_arguments(&calls);
        assert_eq!(sanitized[0].arguments, "{}");
        assert_eq!(sanitized[0].id, "call_1");
        assert_eq!(sanitized[0].name, "listFiles");
    }

    #[test]
    fn sanitize_tool_call_arguments_replaces_a_non_object_json_value() {
        // Valid JSON, but not an object — `arguments` must be an object per
        // the OpenAI function-calling convention.
        let calls = vec![LlmToolCall {
            id: "call_1".to_string(),
            name: "semanticSearch".to_string(),
            arguments: "\"just a string\"".to_string(),
        }];
        let sanitized = sanitize_tool_call_arguments(&calls);
        assert_eq!(sanitized[0].arguments, "{}");
    }

    #[test]
    fn llm_message_round_trips_tool_calls() {
        let message = LlmMessage {
            role: LlmRole::Assistant,
            content: None,
            tool_call_id: None,
            tool_calls: vec![LlmToolCall {
                id: "call_1".to_string(),
                name: "readFile".to_string(),
                arguments: r#"{"path":"a.md"}"#.to_string(),
            }],
        };
        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains(r#""toolCalls":[{"#), "expected toolCalls in {json}");
        let round_tripped: LlmMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, message);
    }

    #[test]
    fn llm_message_omits_tool_calls_when_empty() {
        let message =
            LlmMessage { role: LlmRole::User, content: Some("hi".to_string()), tool_call_id: None, tool_calls: vec![] };
        let json = serde_json::to_string(&message).unwrap();
        assert!(!json.contains("toolCalls"), "expected no toolCalls key in {json}");
    }

    #[test]
    fn chat_stream_result_round_trips_tool_calls() {
        let result = ChatStreamResult {
            text: String::new(),
            reasoning: String::new(),
            usage: None,
            tool_calls: vec![LlmToolCall {
                id: "call_1".to_string(),
                name: "listFiles".to_string(),
                arguments: "{}".to_string(),
            }],
        };
        let json = serde_json::to_string(&result).unwrap();
        let round_tripped: ChatStreamResult = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, result);
    }

    #[test]
    fn chat_stream_result_omits_tool_calls_when_empty() {
        let result =
            ChatStreamResult { text: "hi".to_string(), reasoning: String::new(), usage: None, tool_calls: vec![] };
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("toolCalls"), "expected no toolCalls key in {json}");
    }
}
