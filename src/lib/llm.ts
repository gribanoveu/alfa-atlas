import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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
 * message list (including any system prompt). Resolves with the
 * authoritative full reply text once the stream ends — subscribe via
 * `listenLlmChatDelta` for the live text meanwhile. */
export function streamLlmChat(providerId: string, messages: LlmMessage[]): Promise<string> {
  return invoke<string>("llm_chat_stream", { providerId, messages });
}

/** Fires once per non-empty text chunk while a `streamLlmChat()` call is in
 * flight. */
export function listenLlmChatDelta(
  onDelta: (payload: LlmChatStreamDelta) => void,
): Promise<UnlistenFn> {
  return listen<LlmChatStreamDelta>("llm:chat-stream-delta", (event) => onDelta(event.payload));
}
