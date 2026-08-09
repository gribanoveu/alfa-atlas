import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Task, ToolResult } from "./aiTools";

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
  trustedCertPem: string | null;
  limit: ModelLimit | null;
};

// Mirrors `domain::llm::ModelLimit` — informational token limits for
// whichever model is currently configured, not enforced anywhere in this
// client.
export type ModelLimit = {
  context: number;
  output: number;
};

// Mirrors `domain::llm::LlmSettings`.
export type LlmSettings = {
  activeProviderId: string | null;
  providers: LlmProviderConfig[];
  /** Off by default. When on, `llm_chat_stream` appends every request/
   * response (or error) of every tool-calling round to
   * `~/.atlas/logs/llm.jsonl` — see `infra::llm_debug_log` on the Rust
   * side. Toggled from the LLM settings tab. */
  debugLogging: boolean;
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
  trustedCertPem: string | null;
  limit: ModelLimit | null;
};

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
// `streamLlmChatResume` call resolves with: either a final answer, or a
// round that hit at least one call needing user approval, with nothing in
// that round executed yet.
export type ChatStreamOutcome =
  | { status: "done"; value: ChatStreamResult }
  | { status: "pendingApproval"; value: PendingApproval };

// Mirrors `domain::llm::ToolCallDecision` — one decision on one pending
// call, required for every `PendingToolCall` whose `requiresConfirmation`
// was `true` (validated server-side).
export type ToolCallDecision = {
  id: string;
  approved: boolean;
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

/** Sends one trivial message and returns the reply text — the manual,
 * no-chat-UI-needed way to verify a provider's config, credentials, TLS
 * trust, and HTTP/parsing all actually work end to end. */
export function testLlmConnection(providerId: string): Promise<string> {
  return invoke<string>("llm_test_connection", { providerId });
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
): Promise<ChatStreamOutcome> {
  return invoke<ChatStreamOutcome>("llm_chat_stream", { providerId, messages, todos });
}

/** Continues a conversation paused by a `{status: "pendingApproval"}`
 * outcome from `streamLlmChat` (or a previous `streamLlmChatResume` — a
 * resumed turn can itself pause again on a later round). `history`/
 * `round`/`budgetUsed` must be exactly what that outcome carried, sent
 * back unmodified — the backend keeps no server-side session state
 * between calls. `decisions` must cover exactly the ids of that round's
 * calls whose `requiresConfirmation` was `true`. */
export function streamLlmChatResume(
  providerId: string,
  history: LlmMessage[],
  round: number,
  budgetUsed: number,
  decisions: ToolCallDecision[],
  todos: Task[],
): Promise<ChatStreamOutcome> {
  return invoke<ChatStreamOutcome>("llm_chat_stream_resume", {
    providerId,
    history,
    round,
    budgetUsed,
    decisions,
    todos,
  });
}

/** Fires once per non-empty text chunk while a `streamLlmChat()` call is in
 * flight. */
export function listenLlmChatDelta(
  onDelta: (payload: LlmChatStreamDelta) => void,
): Promise<UnlistenFn> {
  return listen<LlmChatStreamDelta>("llm:chat-stream-delta", (event) => onDelta(event.payload));
}

// Mirrors `commands::llm::ToolCallEventPayload` — fired just before the
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

// Mirrors `commands::llm::ToolResultEventPayload` — fires once the tool
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
