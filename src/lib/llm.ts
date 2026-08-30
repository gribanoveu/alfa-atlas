import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ConversationMode, Task, ToolResult } from "./aiTools";

/** Mirrors `OpenAiCompatibleProvider::{chat_url,models_url}` in
 * `infra/llm_providers/openai_compatible.rs` — the Settings UI uses this to
 * show which endpoints the app will actually call. */
export function resolveOpenAiCompatibleEndpoints(
  baseUrl: string | null | undefined,
): { chat: string; models: string } | null {
  const root = baseUrl?.trim().replace(/\/+$/, "");
  if (!root) return null;
  return {
    chat: `${root}/chat/completions`,
    models: `${root}/models`,
  };
}

// Mirrors `domain::llm::LlmProviderConfig`. Every field but `id` is
// nullable — for a system provider id this is an *override* (`null` means
// "inherit from the compiled-in manifest preset"), for a custom id it's
// the whole definition, typically filled in one field at a time by an
// "add provider" form.
export type LlmProviderConfig = {
  id: string;
  label: string | null;
  baseUrl: string | null;
  model: string | null;
  /** Saved model ids for the picker — populated from Settings. */
  knownModels: string[];
  trustedCertPem: string | null;
  limit: ProviderTokenLimit | null;
  /** When set, replaces bundled preset headers entirely. `null` = inherit. */
  requestHeaders: Record<string, string> | null;
};

// Mirrors `domain::llm::ProviderTokenLimit` — provider-level token limits
// (not per model id), informational only.
export type ProviderTokenLimit = {
  context: number;
  output: number;
};

/** Mirrors `domain::llm::DEFAULT_PROVIDER_TOKEN_LIMIT`. */
export const DEFAULT_PROVIDER_TOKEN_LIMIT: ProviderTokenLimit = {
  context: 200_000,
  output: 30_000,
};

// Mirrors `domain::llm::LlmSettings`.
export type LlmSettings = {
  activeProviderId: string | null;
  providers: LlmProviderConfig[];
  /** Off by default. When on, LLM calls append request/response (or error)
   * lines to `~/.atlas/logs/llm.jsonl` — tool-calling rounds from
   * `llm_chat_stream` and one-shot `llm_chat_once` / memory auto-nap (see
   * `infra::llm_debug_log`). Toggled from the LLM settings tab. */
  debugLogging: boolean;
  /** When true, hides the follow-up suggestion chips shown above the chat
   * after picking a branching suggestion. Never affects the initial
   * suggestion chips in a brand-new, empty conversation. Toggled from the
   * LLM settings tab. */
  followUpSuggestionsDisabled: boolean;
  /** On by default — unlike `debugLogging`, never stores raw document
   * content. When on, every AI-harness tool call gets one redacted row in
   * `~/.atlas/tool_calls.db` (see `infra::tool_call_log` on the Rust side),
   * browsable from Инструменты → Журнал вызовов инструментов. Toggled from
   * the LLM settings tab. */
  toolCallLogging: boolean;
  /** On by default. When on, the chat UI plays a short chime after an
   * assistant turn finishes successfully and, if the window is unfocused,
   * also sends an OS notification. Cancelled / errored turns stay silent.
   * Toggled from the LLM settings tab. */
  taskDoneSoundEnabled: boolean;
  /** On by default. When on, the chat UI plays a short chime when an
   * `askUser` clarifying-question card appears and, if the window is
   * unfocused, also sends an OS notification. Ordinary tool-approval cards
   * stay silent. Toggled from the LLM settings tab. */
  needAnswerSoundEnabled: boolean;
  /** On by default. When off, the status-bar rate-limit chip is hidden
   * and completion tokens are not recorded. The baked-in rule lives in
   * `system_providers.yaml` `rateLimits`. */
  rateLimitEnabled: boolean;
  /** On by default. When on, a background extractor runs after each
   * persisted chat turn and writes lasting facts to OptMem. */
  memoryExtractionEnabled: boolean;
  /** Minimum extractor confidence (0–1) before a candidate fact is stored. */
  memoryConfidenceThreshold: number;
};

// Mirrors `domain::llm::ResolvedLlmProvider` — the merged, ready-to-use
// view (manifest preset + settings override, or a standalone custom
// provider) the picker UI actually renders.
export type ResolvedLlmProvider = {
  id: string;
  label: string;
  baseUrl: string;
  isSystem: boolean;
  model: string | null;
  knownModels: string[];
  trustedCertPem: string | null;
  limit: ProviderTokenLimit | null;
  /** HTTP headers sent with chat/models requests (merged preset + override). */
  requestHeaders: Record<string, string>;
};

/** Substituted with a fresh UUID on each request — mirrors Rust `$uuid`. */
export const LLM_REQUEST_HEADER_UUID = "$uuid";

export function formatLlmRequestHeaders(headers: Record<string, string> | null | undefined): string {
  if (!headers) return "";
  return Object.entries(headers)
    .map(([name, value]) => `${name}: ${value}`)
    .join("\n");
}

export function parseLlmRequestHeaders(text: string): Record<string, string> | null {
  const out: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const colon = trimmed.indexOf(":");
    if (colon <= 0) continue;
    const name = trimmed.slice(0, colon).trim();
    const value = trimmed.slice(colon + 1).trim();
    if (name) out[name] = value;
  }
  return Object.keys(out).length > 0 ? out : null;
}

// Mirrors `domain::llm::LlmModelInfo`.
export type LlmModelInfo = {
  id: string;
};

// Mirrors `domain::llm::LlmRole`/`LlmMessage`.
export type LlmRole = "system" | "user" | "assistant" | "tool";

export type LlmMessage = {
  role: LlmRole;
  content: string | null;
  toolCallId: string | null;
};

export type LlmChatStreamDelta = {
  delta: string;
};

// Mirrors `commands::llm::ChatStreamReasoningPayload` — same shape/lifecycle
// as `LlmChatStreamDelta`, but for a reasoning-capable model's "thinking"
// text, fired ahead of any `LlmChatStreamDelta` for that round. Never fires
// at all for a provider/model that doesn't send `reasoning_content`.
export type LlmChatStreamReasoningDelta = {
  delta: string;
};

export const STEERING_PREFIX =
  "[Уточнение от пользователя, не новое задание — учти в текущей работе]: ";

export type LlmSteeringAppliedEvent = {
  text: string;
};

// Mirrors `domain::llm::ChatUsage` — real token accounting for one completed
// turn, when the provider reports it (requested via `stream_options.
// include_usage`; not every OpenAI-compatible server sends it).
export type ChatUsage = {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
};

// Mirrors `domain::llm::ChatDone` (flattened onto `ChatStreamResult`'s own
// fields on the wire — `text`/`usage` land at the same top-level keys as
// before `todos` existed).
export type ChatStreamResult = {
  text: string;
  /** Authoritative accumulated "thinking" text for the round, when the
   * provider sent `reasoning_content` — the Rust side (`ChatStreamResult`)
   * omits this key entirely (`skip_serializing_if = "String::is_empty"`)
   * for a provider/model that never sends `reasoning_content`, hence
   * optional here rather than `""`. Same safety-net role as `text`:
   * corrects the trailing reasoning block against a dropped
   * `llm:chat-stream-reasoning-delta` event, see `correctTrailingReasoning`
   * in `chatBlocks.ts`. */
  reasoning?: string;
  usage: ChatUsage | null;
  todos: Task[];
};

// Mirrors `domain::llm::PendingToolCall` — one call from a paused round,
// including non-risky calls bundled into the same round (their
// `requiresConfirmation` is `false`; they need no decision and execute
// automatically on resume).
export type PendingToolCall = {
  id: string;
  name: string;
  arguments: string;
  requiresConfirmation: boolean;
};

// Mirrors `domain::llm::PendingApproval`. `history`/`round`/`budgetUsed`
// are opaque to the frontend — never rendered, just round-tripped verbatim
// into `streamLlmChatResume` alongside the user's decisions, since the
// backend keeps no server-side session state between calls.
export type PendingApproval = {
  history: LlmMessage[];
  round: number;
  budgetUsed: number;
  calls: PendingToolCall[];
  todos: Task[];
};

// Mirrors `domain::llm::ChatStreamOutcome` — what one `streamLlmChat`/
// `streamLlmChatResume` call resolves with: either a final answer, a round
// that hit at least one call needing user approval (nothing in that round
// executed yet), or the turn being stopped mid-flight via `cancelLlmChat`
// (same `ChatStreamResult` shape as `"done"` — `text` is whatever had
// streamed in before the stop landed, `""` if it landed between rounds or
// tool calls instead; no tool call from the round in flight at that point
// ever executed).
export type ChatStreamOutcome =
  | { status: "done"; value: ChatStreamResult }
  | { status: "pendingApproval"; value: PendingApproval }
  | { status: "cancelled"; value: ChatStreamResult };

// Mirrors `domain::llm::ToolCallDecision` — one decision on one pending
// call, required for every `PendingToolCall` whose `requiresConfirmation`
// was `true` (validated server-side). For `askUser`, `approved: true` plus
// `answer` carries the structured responses; `approved: false` (or missing
// `answer`) means the user skipped / stopped.
export type AskUserAnswerPayload = {
  answers: Array<{
    questionId: string;
    selectedOptionIds: string[];
    selectedLabels: string[];
    customText: string | null;
  }>;
};

export type ToolCallDecision = {
  id: string;
  approved: boolean;
  answer?: AskUserAnswerPayload;
};

export function getLlmSettings(): Promise<LlmSettings> {
  return invoke<LlmSettings>("llm_get_settings");
}

export function setLlmSettings(settings: LlmSettings): Promise<void> {
  return invoke("llm_set_settings", { settings });
}

/** Every provider available for the picker — every compiled-in system
 * preset (merged with its override, if any) plus every custom provider. */
export function listLlmProviders(): Promise<ResolvedLlmProvider[]> {
  return invoke<ResolvedLlmProvider[]>("llm_list_providers");
}

export function upsertLlmProvider(config: LlmProviderConfig): Promise<void> {
  return invoke("llm_upsert_provider", { config });
}

/** For a system provider id, only clears its settings-layer override
 * (fields revert to the manifest); it can never remove the manifest preset
 * itself — see `infra::llm_provider_manifest`'s doc comment on the Rust
 * side. Don't offer this for system-provider rows in the UI. */
export function removeLlmProvider(providerId: string): Promise<void> {
  return invoke("llm_remove_provider", { providerId });
}

/** Write-only — there is no `getLlmApiKey`, only a status check. */
export function setLlmApiKey(providerId: string, apiKey: string): Promise<void> {
  return invoke("llm_set_api_key", { providerId, apiKey });
}

export function hasLlmApiKey(providerId: string): Promise<boolean> {
  return invoke<boolean>("llm_has_api_key", { providerId });
}

/** Live model list from the provider's own API — used to populate a model
 * picker and to auto-select the first entry when no explicit pin exists. */
export function listLlmModels(providerId: string): Promise<LlmModelInfo[]> {
  return invoke<LlmModelInfo[]>("llm_list_models", { providerId });
}

/** Deduped union — used when persisting a refreshed or manually typed model
 * id into the provider's saved catalog (`knownModels`). */
export function mergeKnownModels(existing: string[], additions: string[]): string[] {
  return [...new Set([...existing, ...additions.map((id) => id.trim()).filter(Boolean)])];
}

/** Sends one trivial message and returns the reply text — the manual,
 * no-chat-UI-needed way to verify a provider's config, credentials, TLS
 * trust, and HTTP/parsing all actually work end to end. */
export function testLlmConnection(providerId: string): Promise<string> {
  return invoke<string>("llm_test_connection", { providerId });
}

// Mirrors `domain::llm::ChatResponse` — a one-shot, non-streaming, tool-free
// completion. `toolCalls` is real on the wire (the type is shared with the
// tool-calling response shape) but always empty here since `llmChatOnce`
// never advertises any tools — omitted from this type as irrelevant to its
// only caller (`useLlmChat`'s history-compaction pass).
export type ChatOnceResponse = {
  content: string | null;
};

/** Mirrors `domain::llm_rate_limit::RateLimitSeverity`. */
export type RateLimitSeverity =
  | "normal"
  | "warning"
  | "critical"
  | "limited"
  | "offHours";

/** Mirrors `domain::llm_rate_limit::RateLimitRelease`. */
export type RateLimitRelease = {
  at: number;
  tokens: number;
};

/** Mirrors `domain::llm_rate_limit::RateLimitSample`. */
export type RateLimitSample = {
  id: string;
  at: number;
  tokens: number;
  expiresAt: number;
};

/** Mirrors `domain::llm_rate_limit::RateLimitSnapshot` — stable UI contract;
 * the frontend must not hard-code window length / limit / working hours. */
export type RateLimitSnapshot = {
  policyId: string;
  label: string;
  used: number;
  remaining: number;
  limit: number;
  windowMs: number | null;
  isEnforced: boolean;
  isLimited: boolean;
  severity: RateLimitSeverity;
  retryUntil: number | null;
  nextReleaseAt: number | null;
  nextEnforceAt: number | null;
  releases: RateLimitRelease[];
  samples: RateLimitSample[];
};

/** Current rate-limit snapshot for the status-bar chip. `policyId === "none"`
 * means the chip should stay hidden (non-EVC provider). */
export function getLlmRateLimitSnapshot(providerId: string): Promise<RateLimitSnapshot> {
  return invoke<RateLimitSnapshot>("llm_rate_limit_snapshot", { providerId });
}

/** One non-streaming, tool-free completion — used by the history-compaction
 * summarization pass (see `src/lib/contextCompaction.ts`), never by the main
 * chat turn loop (see `streamLlmChat` for that). */
export function llmChatOnce(providerId: string, messages: LlmMessage[]): Promise<ChatOnceResponse> {
  return invoke<ChatOnceResponse>("llm_chat_once", { providerId, messages });
}

/** A plain conversation turn, streamed. The caller owns building the full
 * message list (including any system prompt). Resolves with either the
 * final answer (authoritative full reply text, and real token usage if the
 * provider reported one) or a paused round awaiting approval — see
 * `ChatStreamOutcome`. Subscribe via `listenLlmChatDelta` for live text
 * meanwhile. */
export function streamLlmChat(
  providerId: string,
  messages: LlmMessage[],
  todos: Task[],
  activeFilePath: string | null,
  conversationMode: ConversationMode,
): Promise<ChatStreamOutcome> {
  return invoke<ChatStreamOutcome>("llm_chat_stream", {
    providerId,
    messages,
    todos,
    activeFilePath,
    conversationMode,
  });
}

/** Continues a conversation paused by a `{status: "pendingApproval"}`
 * outcome from `streamLlmChat` (or a previous `streamLlmChatResume` — a
 * resumed turn can itself pause again on a later round). `history`/
 * `round`/`budgetUsed` must be exactly what that outcome carried, sent
 * back unmodified — the backend keeps no server-side session state
 * between calls. `decisions` must cover exactly the ids of that round's
 * calls whose `requiresConfirmation` was `true`. `conversationMode` must
 * likewise be exactly what the round paused with — the mode a live picker
 * shows *right now* may have moved on since. */
export function streamLlmChatResume(
  providerId: string,
  history: LlmMessage[],
  round: number,
  budgetUsed: number,
  decisions: ToolCallDecision[],
  todos: Task[],
  activeFilePath: string | null,
  conversationMode: ConversationMode,
): Promise<ChatStreamOutcome> {
  return invoke<ChatStreamOutcome>("llm_chat_stream_resume", {
    providerId,
    history,
    round,
    budgetUsed,
    decisions,
    todos,
    activeFilePath,
    conversationMode,
  });
}

/** Requests that the currently in-flight `streamLlmChat`/
 * `streamLlmChatResume` call (if any) stop as soon as it next checks —
 * mid-stream, between tool-calling rounds, or between individual tool calls
 * within one round; no tool call from the round that was in flight when
 * this lands ever executes, so this also doubles as an emergency brake on a
 * side-effecting tool (`writeFile`/`deleteFile`/...) about to run, not just
 * a way to stop the model mid-sentence. A no-op if nothing is currently
 * running — safe to call speculatively. */
export function cancelLlmChat(): Promise<void> {
  return invoke("llm_cancel_chat");
}

/** Adds guidance to the next fresh model round without interrupting the
 * stream or tool call currently in flight. */
export function steerLlmChat(text: string): Promise<void> {
  return invoke("llm_steer_chat", { text });
}

/** Fires once per non-empty text chunk while a `streamLlmChat()` call is in
 * flight. */
export function listenLlmChatDelta(
  onDelta: (payload: LlmChatStreamDelta) => void,
): Promise<UnlistenFn> {
  return listen<LlmChatStreamDelta>("llm:chat-stream-delta", (event) => onDelta(event.payload));
}

/** Fires once per non-empty `reasoning_content` chunk while a
 * `streamLlmChat()` call is in flight — ahead of any `listenLlmChatDelta`
 * event for that round. Never fires for a provider/model that doesn't send
 * `reasoning_content`. */
export function listenLlmChatReasoningDelta(
  onDelta: (payload: LlmChatStreamReasoningDelta) => void,
): Promise<UnlistenFn> {
  return listen<LlmChatStreamReasoningDelta>("llm:chat-stream-reasoning-delta", (event) => onDelta(event.payload));
}

export function listenLlmSteeringApplied(
  onApplied: (payload: LlmSteeringAppliedEvent) => void,
): Promise<UnlistenFn> {
  return listen<LlmSteeringAppliedEvent>("llm:steering-applied", (event) => onApplied(event.payload));
}

// Mirrors `domain::llm::ToolCallEvent` — fired just before the
// backend executes one tool call inside a `streamLlmChat()` round (the
// whole tool-calling loop is internal to that one call; this and
// `LlmToolResultEvent` are what surface a round's activity mid-flight).
// `id` is the model's own tool-call id, carried through so a later
// `LlmToolResultEvent` can be matched back to the entry this created.
// `arguments` stays a raw JSON string, same as `domain::llm::LlmToolCall`.
export type LlmToolCallEvent = {
  id: string;
  name: string;
  arguments: string;
};

/** Fires while a tool call's arguments are still arriving on the SSE
 * stream — same payload as `listenLlmToolCall`, but `arguments` may be
 * incomplete JSON. The UI upserts a running block immediately so a long
 * `visualize`/`writeFile` argument stream does not look like a hang.
 * Always followed later by `listenLlmToolCall` with the same `id`, unless
 * the turn is cancelled first. */
export function listenLlmToolCallDelta(
  onDelta: (payload: LlmToolCallEvent) => void,
): Promise<UnlistenFn> {
  return listen<LlmToolCallEvent>("llm:tool-call-delta", (event) => onDelta(event.payload));
}

/** Fires immediately before the backend executes one tool call while a
 * `streamLlmChat()` call is in flight — lets the UI show e.g. "Reading
 * docs/x.adoc…" while the (possibly slow) tool execution is actually
 * happening. Always followed by exactly one matching `LlmToolResultEvent`
 * (same `id`) once execution settles. */
export function listenLlmToolCall(
  onToolCall: (payload: LlmToolCallEvent) => void,
): Promise<UnlistenFn> {
  return listen<LlmToolCallEvent>("llm:tool-call", (event) => onToolCall(event.payload));
}

// Mirrors `domain::llm::ToolResultEvent` — fires once the tool
// call started by a matching `LlmToolCallEvent` (same `id`) has settled.
// Exactly one of `result`/`error` is ever non-null.
export type LlmToolResultEvent = {
  id: string;
  result: ToolResult | null;
  error: string | null;
};

/** Fires once a `streamLlmChat()` round's tool call (announced via
 * `listenLlmToolCall`) has settled — lets the UI flip that call's display
 * entry from "running" to "done"/"error" and show what actually happened. */
export function listenLlmToolResult(
  onToolResult: (payload: LlmToolResultEvent) => void,
): Promise<UnlistenFn> {
  return listen<LlmToolResultEvent>("llm:tool-result", (event) => onToolResult(event.payload));
}
