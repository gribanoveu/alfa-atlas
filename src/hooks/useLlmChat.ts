import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AiAccessMode } from "../lib/aiTools";
import {
  buildAccessModeChangeNotice,
  buildAssistantSystemPrompt,
  describeToolActivity,
} from "../lib/assistantConfig";
import {
  listenLlmChatDelta,
  listenLlmToolCall,
  streamLlmChat,
  type ChatUsage,
  type LlmMessage,
} from "../lib/llm";
import { estimateTokenCount } from "../lib/tokens";

export type ChatMessage = {
  id: string;
  role: "user" | "assistant";
  content: string;
  streaming?: boolean;
  failed?: boolean;
  /** Real token usage for this turn, when the provider reported one on the
   * final SSE chunk — only ever set on a completed assistant message. */
  usage?: ChatUsage;
};

/** One `listenLlmToolCall` event turned into a display line. `id` is a
 * fresh uuid per event (not derived from anything on the wire) purely as a
 * stable React key — several tool calls can share the same `name`. */
export type ToolActivityEntry = {
  id: string;
  text: string;
};

/** Owns one conversation's state for the assistant chat panel. The
 * tool-calling loop itself (ReadFile/ListFiles/SemanticSearch) runs
 * entirely inside the backend's `llm_chat_stream` — this hook still does
 * exactly one `streamLlmChat()` call per turn and gets back one resolved
 * reply, unaware of how many model↔tool round trips happened underneath;
 * `toolActivity` is the one bit of that visible here, via a status event.
 *
 * `accessMode` is threaded in (rather than read internally) so the caller's
 * `useAiAccessMode` stays the single source of truth; it's read fresh on
 * every `sendMessage`/`contextTokens` computation, not captured once, so
 * flipping the docs-only/full-repo toggle mid-conversation is reflected on
 * the very next turn. */
export function useLlmChat(providerId: string | null, accessMode: AiAccessMode) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Growing log of this turn's tool calls (e.g. "Читает файл: docs/x.adoc…")
  // — an array, not just the latest one, so a multi-round tool-calling
  // exchange visibly advances (each new call appends a line) instead of
  // silently swapping one static line for another, which reads as frozen
  // even while genuinely still working. Reset at the start of every
  // `sendMessage` and cleared once real text resumes streaming or the turn
  // ends, see the two listeners below.
  const [toolActivity, setToolActivity] = useState<ToolActivityEntry[]>([]);

  // The mode the *previous* request actually went out with — `null` until
  // the first send, so the very first turn never fires a spurious "just
  // switched" notice (the system prompt alone already states the mode
  // correctly for a fresh conversation; the notice exists only for a
  // mid-conversation switch, see `buildAccessModeChangeNotice`).
  const lastSentModeRef = useRef<AiAccessMode | null>(null);

  // Live token deltas — subscribed once for the hook's lifetime, matching
  // `useEmbeddingSetup`'s `listenSyncProgress` effect shape. Appends only
  // to a message that's still `streaming` — a straggler delta arriving
  // after that message was already finalized is a no-op, not a
  // misattribution.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listenLlmChatDelta(({ delta }) => {
      setToolActivity([]);
      setMessages((prev) => {
        const last = prev[prev.length - 1];
        if (!last || last.role !== "assistant" || !last.streaming) return prev;
        return [...prev.slice(0, -1), { ...last, content: last.content + delta }];
      });
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Tool-round status — same effect shape as the delta listener above.
  // Fires before the backend executes each tool call; cleared either here
  // by the next real delta, or in `sendMessage`'s `finally` as a backstop
  // for a turn that ends via error before any delta ever arrives.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listenLlmToolCall(({ name, arguments: args }) => {
      setToolActivity((prev) => [...prev, { id: crypto.randomUUID(), text: describeToolActivity(name, args) }]);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const sendMessage = useCallback(
    async (text: string) => {
      const trimmed = text.trim();
      if (!providerId || sending || !trimmed) return;

      const priorTurns = messages;
      const userMsg: ChatMessage = { id: crypto.randomUUID(), role: "user", content: trimmed };
      const assistantId = crypto.randomUUID();
      setMessages((prev) => [
        ...prev,
        userMsg,
        { id: assistantId, role: "assistant", content: "", streaming: true },
      ]);
      setSending(true);
      setError(null);
      setToolActivity([]);

      // Placed as its own message right before the new user turn (not just
      // folded into the system prompt above) so a mode switch is impossible
      // for the model to skim past — see `buildAccessModeChangeNotice`'s
      // doc comment for why the system prompt's own mode line isn't
      // reliably enough on its own once a conversation has some history.
      const modeChanged = lastSentModeRef.current !== null && lastSentModeRef.current !== accessMode;
      lastSentModeRef.current = accessMode;

      const wireMessages: LlmMessage[] = [
        { role: "system", content: buildAssistantSystemPrompt(accessMode), toolCallId: null },
        ...priorTurns.map((m): LlmMessage => ({ role: m.role, content: m.content, toolCallId: null })),
        ...(modeChanged
          ? [{ role: "system" as const, content: buildAccessModeChangeNotice(accessMode), toolCallId: null }]
          : []),
        { role: "user", content: trimmed, toolCallId: null },
      ];

      try {
        // Authoritative full text overwrites whatever was accumulated from
        // deltas — a safety net in case an event was dropped in transit.
        const { text, usage } = await streamLlmChat(providerId, wireMessages);
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId
              ? { ...m, content: text, streaming: false, usage: usage ?? undefined }
              : m,
          ),
        );
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        setMessages((prev) =>
          prev.map((m) => (m.id === assistantId ? { ...m, streaming: false, failed: true } : m)),
        );
        setError(message);
      } finally {
        setSending(false);
        setToolActivity([]);
      }
    },
    [providerId, sending, messages, accessMode],
  );

  // Context-window usage so far. Every request resends the *entire* message
  // history, so a completed turn's `usage.totalTokens` (prompt + completion,
  // as the provider itself counted it) already is the authoritative total
  // context size at that point — not just a per-turn stat. Once one exists,
  // it anchors the count and only the messages after it (a new user message,
  // or the still-streaming reply) fall back to `estimateTokenCount`. Before
  // any turn has completed (a fresh conversation, or a provider that never
  // reports usage), the whole thing is the character-count estimate, same
  // as before.
  const contextTokens = useMemo(() => {
    const lastUsageIndex = messages.reduce(
      (found, m, i) => (m.usage ? i : found),
      -1,
    );
    if (lastUsageIndex === -1) {
      return (
        estimateTokenCount(buildAssistantSystemPrompt(accessMode)) +
        messages.reduce((sum, m) => sum + estimateTokenCount(m.content), 0)
      );
    }
    const baseline = messages[lastUsageIndex].usage!.totalTokens;
    const tail = messages
      .slice(lastUsageIndex + 1)
      .reduce((sum, m) => sum + estimateTokenCount(m.content), 0);
    return baseline + tail;
  }, [messages, accessMode]);

  return {
    messages,
    sending,
    error,
    sendMessage,
    contextTokens,
    toolActivity,
    systemPrompt: buildAssistantSystemPrompt(accessMode),
  };
}
