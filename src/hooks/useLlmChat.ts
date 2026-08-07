import { useCallback, useEffect, useMemo, useState } from "react";
import { ASSISTANT_SYSTEM_PROMPT } from "../lib/assistantConfig";
import { listenLlmChatDelta, streamLlmChat, type ChatUsage, type LlmMessage } from "../lib/llm";
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

/** Owns one conversation's state for the assistant chat panel — plain
 * request/reply for now, no tool-calling (see AI_HARNESS.md). */
export function useLlmChat(providerId: string | null) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Live token deltas — subscribed once for the hook's lifetime, matching
  // `useEmbeddingSetup`'s `listenSyncProgress` effect shape. Appends only
  // to a message that's still `streaming` — a straggler delta arriving
  // after that message was already finalized is a no-op, not a
  // misattribution.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listenLlmChatDelta(({ delta }) => {
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

      const wireMessages: LlmMessage[] = [
        { role: "system", content: ASSISTANT_SYSTEM_PROMPT, toolCallId: null },
        ...priorTurns.map((m): LlmMessage => ({ role: m.role, content: m.content, toolCallId: null })),
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
      }
    },
    [providerId, sending, messages],
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
        estimateTokenCount(ASSISTANT_SYSTEM_PROMPT) +
        messages.reduce((sum, m) => sum + estimateTokenCount(m.content), 0)
      );
    }
    const baseline = messages[lastUsageIndex].usage!.totalTokens;
    const tail = messages
      .slice(lastUsageIndex + 1)
      .reduce((sum, m) => sum + estimateTokenCount(m.content), 0);
    return baseline + tail;
  }, [messages]);

  return { messages, sending, error, sendMessage, contextTokens, systemPrompt: ASSISTANT_SYSTEM_PROMPT };
}
