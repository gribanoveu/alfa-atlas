import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AiAccessMode, LlmToolDefinition } from "../lib/aiTools";
import { buildAccessModeChangeNotice, buildAssistantSystemPrompt } from "../lib/assistantConfig";
import type { SpecsRepoInfo } from "../lib/openapi";
import {
  appendDeltaToBlocks,
  appendToolCallBlock,
  chatMessageToPlainText,
  correctTrailingText,
  markRunningToolCallsAsInterrupted,
  settleToolCallBlock,
  updateLastAssistantBlocks,
  type ChatMessage,
} from "../lib/chatBlocks";
import {
  listenLlmChatDelta,
  listenLlmToolCall,
  listenLlmToolResult,
  streamLlmChat,
  streamLlmChatResume,
  type LlmMessage,
  type PendingToolCall,
  type ToolCallDecision,
} from "../lib/llm";
import { estimateTokenCount } from "../lib/tokens";

export type { ChatMessage, MessageBlock, TextBlock, ToolCallBlock, ToolCallStatus } from "../lib/chatBlocks";

/** One paused round awaiting a user decision, trimmed to just the calls
 * that still need one — a call whose tool the user already marked "don't
 * ask again this conversation" never reaches this (see `useLlmChat`'s
 * `trustedToolsRef`), so the review UI only ever shows what genuinely needs
 * a fresh look. */
export type PendingReview = {
  round: number;
  calls: PendingToolCall[];
};

/** What the review UI hands back once the user has decided every call in a
 * `PendingReview` — `trustToolNames` lists which of those calls' tool
 * names (e.g. `"writeFile"`) should stop prompting for the rest of this
 * conversation. */
export type ApprovalSubmission = {
  decisions: ToolCallDecision[];
  trustToolNames: string[];
};

/** Owns one conversation's state for the assistant chat panel. The
 * tool-calling loop itself (ReadFile/ListFiles/SemanticSearch) runs
 * entirely inside the backend's `llm_chat_stream` — this hook still does
 * exactly one `streamLlmChat()` call per turn and gets back one resolved
 * reply, but now reconstructs every round's activity as permanent
 * `MessageBlock`s on the in-flight assistant message via three live event
 * listeners (delta / tool-call / tool-result), rather than treating tool
 * activity as transient status that's discarded once text resumes.
 *
 * `accessMode` is threaded in (rather than read internally) so the caller's
 * `useAiAccessMode` stays the single source of truth; it's read fresh on
 * every `sendMessage`/`contextTokens` computation, not captured once, so
 * flipping the docs-only/full-repo toggle mid-conversation is reflected on
 * the very next turn. `specsRepoInfo` is likewise threaded in rather than
 * detected here — the caller's own `useSpecsRepo` (App.tsx) already runs
 * it once per `repoRoot`, so this hook just forwards whatever it found
 * into the system prompt's "Current project type" line. `toolDefinitions`
 * is likewise threaded in from the caller's own `useToolDefinitions`,
 * so the system prompt's "## Tool usage" section is generated from the
 * same live registry the backend uses for real function-calling. */
export function useLlmChat(
  providerId: string | null,
  accessMode: AiAccessMode,
  specsRepoInfo: SpecsRepoInfo | null,
  toolDefinitions: LlmToolDefinition[],
) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pendingReview, setPendingReview] = useState<PendingReview | null>(null);

  // The mode the *previous* request actually went out with — `null` until
  // the first send, so the very first turn never fires a spurious "just
  // switched" notice (the system prompt alone already states the mode
  // correctly for a fresh conversation; the notice exists only for a
  // mid-conversation switch, see `buildAccessModeChangeNotice`).
  const lastSentModeRef = useRef<AiAccessMode | null>(null);

  // Tool names (e.g. `"writeFile"`) the user has ticked "don't ask again
  // this conversation" for — checked before ever showing the review modal
  // for a later round. Scoped per tool, not blanket: trusting `writeFile`
  // doesn't silently pre-approve a later `requestFullRepoAccess`. Lives for
  // the panel's mounted lifetime, same as `messages` itself — there's no
  // separate "clear conversation" action yet to reset it on, so it can't
  // outlive what the user perceives as a fresh chat any more than the
  // transcript already does.
  const trustedToolsRef = useRef<Set<string>>(new Set());

  // Resolves the promise `awaitApproval` handed to `sendMessage`'s resume
  // loop, once the review UI calls `submitApprovalDecisions`. `null`
  // whenever `pendingReview` is `null`.
  const approvalResolverRef = useRef<((result: ApprovalSubmission) => void) | null>(null);

  // Which call ids the resume about to run auto-approved via
  // `trustedToolsRef` (as opposed to the user just having clicked Approve)
  // — read by the `listenLlmToolCall` effect below so the resulting block
  // can carry `autoApproved: true` for display. Reassigned right before
  // each `streamLlmChatResume`, since `TOOL_CALL_EVENT`s for that call only
  // start arriving once it's in flight.
  const autoApprovedIdsRef = useRef<Set<string>>(new Set());

  const awaitApproval = useCallback((round: number, calls: PendingToolCall[]): Promise<ApprovalSubmission> => {
    return new Promise((resolve) => {
      approvalResolverRef.current = resolve;
      setPendingReview({ round, calls });
    });
  }, []);

  const submitApprovalDecisions = useCallback((submission: ApprovalSubmission) => {
    approvalResolverRef.current?.(submission);
    approvalResolverRef.current = null;
    setPendingReview(null);
  }, []);

  // Live token deltas — subscribed once for the hook's lifetime, matching
  // `useEmbeddingSetup`'s `listenSyncProgress` effect shape. Appends only
  // to a message that's still `streaming` (via `updateLastAssistantBlocks`'s
  // guard) — a straggler delta arriving after that message was already
  // finalized is a no-op, not a misattribution.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listenLlmChatDelta(({ delta }) => {
      setMessages((prev) => updateLastAssistantBlocks(prev, (blocks) => appendDeltaToBlocks(blocks, delta)));
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Fires just before the backend executes each tool call — pushes a new
  // permanent "running" block onto the in-flight assistant message (closing
  // off whatever text preceded it).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listenLlmToolCall(({ id, name, arguments: argumentsJson }) => {
      const autoApproved = autoApprovedIdsRef.current.has(id);
      setMessages((prev) =>
        updateLastAssistantBlocks(prev, (blocks) =>
          appendToolCallBlock(blocks, { id, name, argumentsJson, autoApproved }),
        ),
      );
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Fires once a tool call announced above has settled — flips that block's
  // status and attaches its result/error. The block is never removed.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listenLlmToolResult(({ id, result, error: toolError }) => {
      setMessages((prev) =>
        updateLastAssistantBlocks(prev, (blocks) => settleToolCallBlock(blocks, { id, result, error: toolError })),
      );
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
        { id: assistantId, role: "assistant", blocks: [], streaming: true },
      ]);
      setSending(true);
      setError(null);

      // Placed as its own message right before the new user turn (not just
      // folded into the system prompt above) so a mode switch is impossible
      // for the model to skim past — see `buildAccessModeChangeNotice`'s
      // doc comment for why the system prompt's own mode line isn't
      // reliably enough on its own once a conversation has some history.
      const modeChanged = lastSentModeRef.current !== null && lastSentModeRef.current !== accessMode;
      lastSentModeRef.current = accessMode;

      const wireMessages: LlmMessage[] = [
        { role: "system", content: buildAssistantSystemPrompt(accessMode, specsRepoInfo, toolDefinitions), toolCallId: null },
        ...priorTurns.map((m): LlmMessage => ({ role: m.role, content: chatMessageToPlainText(m), toolCallId: null })),
        ...(modeChanged
          ? [{ role: "system" as const, content: buildAccessModeChangeNotice(accessMode), toolCallId: null }]
          : []),
        { role: "user", content: trimmed, toolCallId: null },
      ];

      try {
        // Most turns resolve in one round trip. A round that hits a call
        // needing confirmation (`writeFile`/`requestFullRepoAccess`) comes
        // back as `pendingApproval` instead — nothing in it executed yet —
        // and this loop collects a decision for each call (skipping the
        // modal entirely for tool names already trusted this conversation)
        // before resuming, potentially several times if later rounds pause
        // again.
        let outcome = await streamLlmChat(providerId, wireMessages);
        while (outcome.status === "pendingApproval") {
          const { history, round, calls } = outcome.value;
          const risky = calls.filter((c) => c.requiresConfirmation);
          const autoApprovedIds = new Set<string>();
          const needsPrompt = risky.filter((c) => {
            if (trustedToolsRef.current.has(c.name)) {
              autoApprovedIds.add(c.id);
              return false;
            }
            return true;
          });

          let decisions: ToolCallDecision[];
          if (needsPrompt.length === 0) {
            decisions = risky.map((c) => ({ id: c.id, approved: true }));
          } else {
            const submission = await awaitApproval(round, needsPrompt);
            submission.trustToolNames.forEach((name) => trustedToolsRef.current.add(name));
            decisions = [
              ...submission.decisions,
              ...risky.filter((c) => autoApprovedIds.has(c.id)).map((c) => ({ id: c.id, approved: true })),
            ];
          }

          autoApprovedIdsRef.current = autoApprovedIds;
          outcome = await streamLlmChatResume(providerId, history, round, decisions);
        }

        // Authoritative full text of the *final* round corrects only the
        // trailing text block — see `correctTrailingText`'s doc comment for
        // why that's always the right (and only) block it can apply to.
        const { text, usage } = outcome.value;
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId && m.role === "assistant"
              ? { ...m, blocks: correctTrailingText(m.blocks, text), streaming: false, usage: usage ?? undefined }
              : m,
          ),
        );
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId && m.role === "assistant"
              ? { ...m, blocks: markRunningToolCallsAsInterrupted(m.blocks), streaming: false, failed: true }
              : m,
          ),
        );
        setError(message);
      } finally {
        setSending(false);
      }
    },
    [providerId, sending, messages, accessMode, specsRepoInfo, toolDefinitions, awaitApproval],
  );

  // Context-window usage so far. Every request resends the *entire* message
  // history, so a completed turn's `usage.totalTokens` (prompt + completion,
  // as the provider itself counted it) already is the authoritative total
  // context size at that point — not just a per-turn stat. Once one exists,
  // it anchors the count and only the messages after it (a new user message,
  // or the still-streaming reply) fall back to `estimateTokenCount`. Before
  // any turn has completed (a fresh conversation, or a provider that never
  // reports usage), the whole thing is the character-count estimate, same
  // as before. Expressed as a `forEach` scan (not `reduce`-then-reindex)
  // since `usage` only exists on the assistant arm of `ChatMessage`'s
  // discriminated union — carrying the found value along with the index in
  // one pass avoids re-narrowing the role a second time.
  const contextTokens = useMemo(() => {
    let lastUsageIndex = -1;
    let lastUsageTotal: number | null = null;
    messages.forEach((m, i) => {
      if (m.role === "assistant" && m.usage) {
        lastUsageIndex = i;
        lastUsageTotal = m.usage.totalTokens;
      }
    });
    if (lastUsageIndex === -1 || lastUsageTotal === null) {
      return (
        estimateTokenCount(buildAssistantSystemPrompt(accessMode, specsRepoInfo, toolDefinitions)) +
        messages.reduce((sum, m) => sum + estimateTokenCount(chatMessageToPlainText(m)), 0)
      );
    }
    const tail = messages
      .slice(lastUsageIndex + 1)
      .reduce((sum, m) => sum + estimateTokenCount(chatMessageToPlainText(m)), 0);
    return lastUsageTotal + tail;
  }, [messages, accessMode, specsRepoInfo, toolDefinitions]);

  return {
    messages,
    sending,
    error,
    sendMessage,
    contextTokens,
    systemPrompt: buildAssistantSystemPrompt(accessMode, specsRepoInfo, toolDefinitions),
    pendingReview,
    submitApprovalDecisions,
  };
}
