import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getAutoApprovedTools, setToolAutoApproved, type AiAccessMode, type LlmToolDefinition, type Task } from "../lib/aiTools";
import {
  buildAccessModeChangeNotice,
  buildAssistantSystemPrompt,
  buildTodoContextBlock,
  TOOL_APPROVAL_TIMEOUT_MS,
} from "../lib/assistantConfig";
import type { SpecsRepoInfo } from "../lib/openapi";
import {
  appendDeltaToBlocks,
  appendPendingApprovalBlock,
  appendToolCallBlock,
  chatMessageToPlainText,
  correctTrailingText,
  markRunningToolCallsAsInterrupted,
  settleToolCallBlock,
  updateLastAssistantBlocks,
  type ChatMessage,
} from "../lib/chatBlocks";
import {
  cancelLlmChat,
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
 * same live registry the backend uses for real function-calling.
 *
 * `initialMessages`/`initialTodos` seed this hook's conversation — the
 * caller (`AssistantConversation`) is expected to remount this hook (via a
 * `key={chatId}` on its own component) whenever the active chat changes,
 * so a plain initial value is enough; this never needs to react to either
 * changing in place. `onTurnSettled` fires once per `sendMessage` call, on
 * both the success and error paths, with the turn's final `messages`/
 * `todos` snapshot — the caller's `useChatHistory` uses it to persist the
 * conversation (including the todo checklist, which otherwise only ever
 * lived in this hook's own `todoListRef`). */
export function useLlmChat(
  providerId: string | null,
  accessMode: AiAccessMode,
  specsRepoInfo: SpecsRepoInfo | null,
  toolDefinitions: LlmToolDefinition[],
  docsRootRelativeToRepo: string | null,
  initialMessages: ChatMessage[],
  initialTodos: Task[],
  onTurnSettled: (messages: ChatMessage[], todos: Task[]) => void,
  activeFilePath: string | null,
) {
  const [messages, setMessages] = useState<ChatMessage[]>(initialMessages);
  const [sending, setSending] = useState(false);

  // The mode the *previous* request actually went out with — `null` until
  // the first send, so the very first turn never fires a spurious "just
  // switched" notice (the system prompt alone already states the mode
  // correctly for a fresh conversation; the notice exists only for a
  // mid-conversation switch, see `buildAccessModeChangeNotice`).
  const lastSentModeRef = useRef<AiAccessMode | null>(null);

  // Tool names (e.g. `"writeFile"`) the user has ticked "don't ask again"
  // for — checked before ever showing an approval card for a later round.
  // Scoped per tool, not blanket: trusting `writeFile` doesn't silently
  // pre-approve a later `requestFullRepoAccess`. Seeded on mount from the
  // project's persisted `ai_auto_approved_tools` (see the effect below), so
  // a choice made in one chat carries into every later chat on this repo
  // rather than living only for this panel's mounted lifetime.
  const trustedToolsRef = useRef<Set<string>>(new Set());

  // Loads whichever tools this project already has persisted as "always
  // allow" (from a previous chat, or a previous session) and merges them
  // into `trustedToolsRef` before the user sends anything — so a round that
  // would otherwise pause for confirmation is silently auto-approved from
  // the very first turn, not just after the user re-clicks "Разрешать
  // всегда" in this chat. Runs once per mount, matching `trustedToolsRef`'s
  // own per-chat-mount lifetime; a project switch remounts this hook (new
  // `providerId`/chat) so there's no stale cross-project leak to guard.
  useEffect(() => {
    let cancelled = false;
    void getAutoApprovedTools().then((tools) => {
      if (cancelled) return;
      for (const tool of tools) trustedToolsRef.current.add(tool);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  // This turn's task checklist, owned by the frontend exactly like
  // `messages` itself — the backend keeps no server-side session state
  // between calls (see `commands::llm`'s doc comment), so this is the
  // entire source of truth between turns. Sent to Rust on every
  // `streamLlmChat`/`streamLlmChatResume` call, overwritten from the
  // response after each one (both the `"done"` case and every iteration of
  // the `"pendingApproval"` resume loop below) — same lifetime as
  // `trustedToolsRef`: lives for the panel's mounted lifetime, reset
  // automatically when `AssistantConversation` remounts on chat switch
  // (`key={chatHistory.currentChatId}` in `AssistantPanel.tsx`), no manual
  // reset needed.
  //
  // Two views onto the same value: `todoListRef` is read synchronously
  // inside `sendMessage`'s async flow (a plain `useState` value can't be
  // read "as of right now" mid-function — updates only land on the next
  // render), while `todos` (state) exists purely so `TodoProgressWidget`
  // re-renders when the list changes. `setTodos` below keeps both in sync
  // on every mutation; nothing should assign to either independently.
  const todoListRef = useRef<Task[]>(initialTodos);
  const [todos, setTodosState] = useState<Task[]>(initialTodos);
  const setTodos = useCallback((next: Task[]) => {
    todoListRef.current = next;
    setTodosState(next);
  }, []);

  // The in-flight batch of pending decisions, if any — `decide` is what
  // `AssistantToolCallBlock`'s Approve/Deny buttons (via `decideToolCall`
  // below) and each call's own `TOOL_APPROVAL_TIMEOUT_MS` timeout both
  // funnel through, so a manual click and an expired timer settle a call
  // through the exact same path. `denyAll` is `stopChat`'s hook into the
  // same mechanism — see its own doc comment below. `null` whenever no
  // round is currently paused awaiting decisions.
  const activeApprovalRef = useRef<{
    decide: (id: string, approved: boolean, trust: boolean) => void;
    denyAll: () => void;
  } | null>(null);

  // Which call ids the resume about to run auto-approved via
  // `trustedToolsRef` (as opposed to the user just having clicked Approve)
  // — read by the `listenLlmToolCall` effect below so the resulting block
  // can carry `autoApproved: true` for display. Reassigned right before
  // each `streamLlmChatResume`, since `TOOL_CALL_EVENT`s for that call only
  // start arriving once it's in flight.
  const autoApprovedIdsRef = useRef<Set<string>>(new Set());

  /** Shows every call in `calls` inline in the transcript right away as a
   * `"pendingApproval"` card (see `AssistantToolCallBlock`), each counting
   * down toward the same `deadlineAt`. Resolves once every call has a
   * decision — a manual Approve/Deny via `decideToolCall`, or its own timer
   * expiring first, whichever comes first, both funneled through the same
   * `decide` closure so there's exactly one place that finalizes a call's
   * outcome. */
  const collectDecisions = useCallback((calls: PendingToolCall[]): Promise<ToolCallDecision[]> => {
    return new Promise((resolve) => {
      const decided = new Map<string, boolean>();
      const timers = new Map<string, ReturnType<typeof setTimeout>>();
      const deadlineAt = Date.now() + TOOL_APPROVAL_TIMEOUT_MS;

      const decide = (id: string, approved: boolean, trust: boolean) => {
        if (decided.has(id) || !calls.some((c) => c.id === id)) return;
        decided.set(id, approved);
        const timer = timers.get(id);
        if (timer !== undefined) clearTimeout(timer);
        if (trust && approved) {
          const call = calls.find((c) => c.id === id);
          if (call) {
            trustedToolsRef.current.add(call.name);
            void setToolAutoApproved(call.name, true);
          }
        }
        if (decided.size === calls.length) {
          activeApprovalRef.current = null;
          resolve(calls.map((c) => ({ id: c.id, approved: decided.get(c.id) ?? false })));
        }
      };

      activeApprovalRef.current = { decide, denyAll: () => calls.forEach((c) => decide(c.id, false, false)) };

      calls.forEach((c) => {
        timers.set(
          c.id,
          setTimeout(() => decide(c.id, false, false), TOOL_APPROVAL_TIMEOUT_MS),
        );
      });

      setMessages((prev) =>
        updateLastAssistantBlocks(prev, (blocks) =>
          calls.reduce(
            (acc, c) =>
              appendPendingApprovalBlock(acc, { id: c.id, name: c.name, argumentsJson: c.arguments, deadlineAt }),
            blocks,
          ),
        ),
      );
    });
  }, []);

  /** Passed down to `AssistantToolCallBlock`'s Approve/Deny buttons for a
   * `"pendingApproval"` card. A no-op once that call already has a decision
   * (its own timeout fired first, or the button was somehow clicked twice)
   * — `collectDecisions`'s `decide` already guards this, this is just the
   * stable callback identity the UI holds onto. */
  const decideToolCall = useCallback((id: string, approved: boolean, trust: boolean) => {
    activeApprovalRef.current?.decide(id, approved, trust);
  }, []);

  /** Stops the in-flight turn, wherever it currently is: mid-stream,
   * between tool-calling rounds, or waiting on a `"pendingApproval"` card.
   * `cancelLlmChat()` sets the backend's cancel flag so the next checkpoint
   * `run_tool_loop` hits resolves `{status: "cancelled"}` instead of
   * continuing (see `commands::llm::run_tool_loop`'s doc comment on the
   * Rust side for exactly where those checkpoints are) — but if a
   * `"pendingApproval"` card is showing right now, nothing on the backend
   * is actually running to reach one; `denyAll` unblocks `collectDecisions`'s
   * pending promise immediately (bypassing the countdown) so `sendMessage`'s
   * `while` loop proceeds straight to `streamLlmChatResume`, which then
   * hits the very first checkpoint before executing any of those
   * (already-denied) calls. Safe to call when nothing is in flight — both
   * calls are no-ops in that case. */
  const stopChat = useCallback(() => {
    void cancelLlmChat();
    activeApprovalRef.current?.denyAll();
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

      // Placed as its own message right before the new user turn (not just
      // folded into the system prompt above) so a mode switch is impossible
      // for the model to skim past — see `buildAccessModeChangeNotice`'s
      // doc comment for why the system prompt's own mode line isn't
      // reliably enough on its own once a conversation has some history.
      const modeChanged = lastSentModeRef.current !== null && lastSentModeRef.current !== accessMode;
      lastSentModeRef.current = accessMode;

      const todoBlock = buildTodoContextBlock(todoListRef.current);

      const wireMessages: LlmMessage[] = [
        {
          role: "system",
          content: buildAssistantSystemPrompt(accessMode, specsRepoInfo, toolDefinitions, docsRootRelativeToRepo),
          toolCallId: null,
        },
        ...priorTurns.map((m): LlmMessage => ({ role: m.role, content: chatMessageToPlainText(m), toolCallId: null })),
        ...(modeChanged
          ? [
              {
                role: "system" as const,
                content: buildAccessModeChangeNotice(accessMode, docsRootRelativeToRepo),
                toolCallId: null,
              },
            ]
          : []),
        ...(todoBlock ? [{ role: "system" as const, content: todoBlock, toolCallId: null }] : []),
        { role: "user", content: trimmed, toolCallId: null },
      ];

      try {
        // Most turns resolve in one round trip. A round that hits a call
        // needing confirmation (`writeFile`/`requestFullRepoAccess`) comes
        // back as `pendingApproval` instead — nothing in it executed yet —
        // and this loop collects a decision for each call (skipping the
        // card entirely for tool names already trusted, whether from this
        // chat or persisted from an earlier one on this project) before
        // resuming, potentially several times if later rounds pause again.
        let outcome = await streamLlmChat(providerId, wireMessages, todoListRef.current, activeFilePath);
        while (outcome.status === "pendingApproval") {
          const { history, round, budgetUsed, calls, todos: updatedTodos } = outcome.value;
          setTodos(updatedTodos);
          const risky = calls.filter((c) => c.requiresConfirmation);
          const autoApprovedIds = new Set<string>();
          const needsDecision = risky.filter((c) => {
            if (trustedToolsRef.current.has(c.name)) {
              autoApprovedIds.add(c.id);
              return false;
            }
            return true;
          });

          const decisions =
            needsDecision.length === 0
              ? risky.map((c) => ({ id: c.id, approved: true }))
              : [
                  ...(await collectDecisions(needsDecision)),
                  ...risky.filter((c) => autoApprovedIds.has(c.id)).map((c) => ({ id: c.id, approved: true })),
                ];

          autoApprovedIdsRef.current = autoApprovedIds;
          outcome = await streamLlmChatResume(
            providerId,
            history,
            round,
            budgetUsed,
            decisions,
            todoListRef.current,
            activeFilePath,
          );
        }

        // Authoritative full text of the *final* round corrects only the
        // trailing text block — see `correctTrailingText`'s doc comment for
        // why that's always the right (and only) block it can apply to.
        // Same handling for `"cancelled"` as `"done"` (both carry the same
        // `ChatStreamResult` shape) — the only difference is the extra
        // `cancelled` flag for display and sweeping any pending-approval
        // card `stopChat` auto-denied but that never got its settling
        // event, since `run_tool_loop` returned before reaching it.
        const stoppedByUser = outcome.status === "cancelled";
        const { text, usage, todos: finalTodos } = outcome.value;
        setTodos(finalTodos);
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId && m.role === "assistant"
              ? {
                  ...m,
                  blocks: stoppedByUser
                    ? markRunningToolCallsAsInterrupted(correctTrailingText(m.blocks, text), "Остановлено пользователем")
                    : correctTrailingText(m.blocks, text),
                  streaming: false,
                  usage: usage ?? undefined,
                  cancelled: stoppedByUser,
                }
              : m,
          ),
        );
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId && m.role === "assistant"
              ? {
                  ...m,
                  blocks: markRunningToolCallsAsInterrupted(m.blocks),
                  streaming: false,
                  failed: true,
                  errorMessage: message,
                }
              : m,
          ),
        );
      } finally {
        setSending(false);
        // Reads the true final state for this turn via a functional-update
        // "peek" — the `try`/`catch` block's own `setMessages` call and
        // this one happen synchronously in the same tick (no `await`
        // between them), so React's batching applies them to the update
        // queue in order; this updater sees exactly what the turn ended
        // with, covering both the success and error paths in one place.
        setMessages((prev) => {
          onTurnSettled(prev, todoListRef.current);
          return prev;
        });
      }
    },
    [
      providerId,
      sending,
      messages,
      accessMode,
      specsRepoInfo,
      toolDefinitions,
      docsRootRelativeToRepo,
      collectDecisions,
      onTurnSettled,
      setTodos,
      activeFilePath,
    ],
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
        estimateTokenCount(
          buildAssistantSystemPrompt(accessMode, specsRepoInfo, toolDefinitions, docsRootRelativeToRepo),
        ) + messages.reduce((sum, m) => sum + estimateTokenCount(chatMessageToPlainText(m)), 0)
      );
    }
    const tail = messages
      .slice(lastUsageIndex + 1)
      .reduce((sum, m) => sum + estimateTokenCount(chatMessageToPlainText(m)), 0);
    return lastUsageTotal + tail;
  }, [messages, accessMode, specsRepoInfo, toolDefinitions, docsRootRelativeToRepo]);

  /** Bulk version of what a model-driven `todo update` already does one
   * task at a time — marks every non-terminal task `cancelled`, same status,
   * not a different deletion semantic. Explicitly persists via
   * `onTurnSettled` (the only place `useChatHistory`'s `saveChat` is invoked
   * outside `sendMessage`'s own `finally` block), since a button firing
   * between turns has no other path to survive a reload otherwise. */
  const clearTodos = useCallback(() => {
    const next = todoListRef.current.map((t) =>
      t.status === "pending" || t.status === "inProgress" ? { ...t, status: "cancelled" as const } : t,
    );
    setTodos(next);
    onTurnSettled(messages, next);
  }, [messages, onTurnSettled, setTodos]);

  return {
    messages,
    sending,
    sendMessage,
    contextTokens,
    todos,
    clearTodos,
    systemPrompt: buildAssistantSystemPrompt(accessMode, specsRepoInfo, toolDefinitions, docsRootRelativeToRepo),
    decideToolCall,
    stopChat,
  };
}
