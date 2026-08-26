import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  getAutoApprovedTools,
  getMemoryWake,
  onAutoApprovedToolsChange,
  setToolAutoApproved,
  type AiAccessMode,
  type ConversationMode,
  type LlmToolDefinition,
  type Task,
} from "../lib/aiTools";
import {
  buildAccessModeChangeNotice,
  buildActiveFileContextBlock,
  buildActivePlanContextBlock,
  buildCompactionSummaryBlock,
  buildHistoryCompactionPrompt,
  buildModeChangeNotice,
  buildSystemPromptForConversationMode,
  buildTodoContextBlock,
  buildMemoryContextBlock,
  sliceMessagesForPlanExecution,
  CONTEXT_COMPACTION_KEEP_LAST_MESSAGES,
  CONTEXT_COMPACTION_RETRY_KEEP_LAST_MESSAGES,
  TOOL_APPROVAL_TIMEOUT_MS,
} from "../lib/assistantConfig";
import type { SpecsRepoInfo } from "../lib/openapi";
import {
  appendDeltaToBlocks,
  appendPendingApprovalBlock,
  appendReasoningDeltaToBlocks,
  appendToolCallBlock,
  chatMessageToPlainText,
  correctTrailingReasoning,
  correctTrailingText,
  markRunningToolCallsAsInterrupted,
  settleToolCallBlock,
  updateLastAssistantBlocks,
  type ChatMessage,
} from "../lib/chatBlocks";
import {
  describeMessageForCompaction,
  formatCompactionNoticeText,
  isCacheValid,
  isContextLengthError,
  planCompaction,
  realMessages,
  shouldCompact,
  type CompactionCache,
} from "../lib/contextCompaction";
import {
  cancelLlmChat,
  listenLlmChatDelta,
  listenLlmChatReasoningDelta,
  listenLlmToolCall,
  listenLlmToolResult,
  llmChatOnce,
  streamLlmChat,
  streamLlmChatResume,
  type ChatStreamOutcome,
  type LlmMessage,
  type AskUserAnswerPayload,
  type PendingApproval,
  type PendingToolCall,
  type ToolCallDecision,
} from "../lib/llm";
import { playNeedAnswerSound, playTaskDoneSound } from "../lib/assistantSounds";
import { planGet, type PlanRecord } from "../lib/plans";
import { estimateTokenCount } from "../lib/tokens";

export type { ChatMessage, MessageBlock, ReasoningBlock, TextBlock, ToolCallBlock, ToolCallStatus } from "../lib/chatBlocks";

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
 * lived in this hook's own `todoListRef`).
 *
 * `initialPendingResume`/`onTurnPaused` cover the case where this chat was
 * last saved *mid-turn*, paused on a tool-approval/`askUser` card that was
 * never resolved before the app fully closed (not just the panel being
 * hidden within one running session — see `RightDock`'s keep-mounted fix
 * for that case, which never unmounts this hook at all). `onTurnPaused`
 * fires every time a round actually pauses (mirroring `onTurnSettled`, but
 * per-pause rather than per-turn) so `useChatHistory` can persist enough to
 * resume later; `initialPendingResume`, when set, replays that exact pause
 * once on mount so the already-restored pending-approval card (it's part of
 * `initialMessages`, nothing new to render) becomes answerable again. */
export function useLlmChat(
  providerId: string | null,
  contextLimit: number | null,
  accessMode: AiAccessMode,
  conversationMode: ConversationMode,
  specsRepoInfo: SpecsRepoInfo | null,
  toolDefinitions: LlmToolDefinition[],
  docsRootRelativeToRepo: string | null,
  initialMessages: ChatMessage[],
  initialTodos: Task[],
  initialActivePlanId: string | null,
  initialPendingResume: PendingApproval | null,
  onTurnSettled: (messages: ChatMessage[], todos: Task[], activePlanId: string | null) => void,
  onTurnPaused: (messages: ChatMessage[], todos: Task[], activePlanId: string | null, pendingResume: PendingApproval) => void,
  activeFilePath: string | null,
  taskDoneSoundEnabled: boolean,
  needAnswerSoundEnabled: boolean,
) {
  const [messages, setMessages] = useState<ChatMessage[]>(initialMessages);
  const [sending, setSending] = useState(false);

  // Sound toggles live in refs so `collectDecisions` (empty deps — one
  // stable Promise factory for the panel's lifetime) always reads the
  // latest Settings-tab value without being recreated on every toggle.
  const taskDoneSoundEnabledRef = useRef(taskDoneSoundEnabled);
  const needAnswerSoundEnabledRef = useRef(needAnswerSoundEnabled);
  useEffect(() => {
    taskDoneSoundEnabledRef.current = taskDoneSoundEnabled;
  }, [taskDoneSoundEnabled]);
  useEffect(() => {
    needAnswerSoundEnabledRef.current = needAnswerSoundEnabled;
  }, [needAnswerSoundEnabled]);

  // The mode the *previous* request actually went out with — `null` until
  // the first send, so the very first turn never fires a spurious "just
  // switched" notice (the system prompt alone already states the mode
  // correctly for a fresh conversation; the notice exists only for a
  // mid-conversation switch, see `buildAccessModeChangeNotice`).
  const lastSentModeRef = useRef<AiAccessMode | null>(null);

  // Same "did it change since the last turn we actually sent" diff, for
  // `conversationMode` — see `buildModeChangeNotice`. Independent of
  // `lastSentModeRef` above: an access-mode switch and a conversation-mode
  // switch are unrelated axes and can happen on the same turn or separately.
  const lastSentConversationModeRef = useRef<ConversationMode | null>(null);

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
  //
  // Also subscribes to every later `setToolAutoApproved` call for as long as
  // this hook stays mounted — in particular a revoke from `PermissionsTab`
  // while this exact chat panel is already open. Without this, revoking a
  // tool only updates the persisted project config; this panel's
  // `trustedToolsRef` would keep the stale entry and go on silently
  // auto-approving that "revoked" tool for the rest of its mounted lifetime.
  useEffect(() => {
    let cancelled = false;
    void getAutoApprovedTools().then((tools) => {
      if (cancelled) return;
      for (const tool of tools) trustedToolsRef.current.add(tool);
    });
    const unsubscribe = onAutoApprovedToolsChange(({ tool, autoApproved }) => {
      if (autoApproved) trustedToolsRef.current.add(tool);
      else trustedToolsRef.current.delete(tool);
    });
    return () => {
      cancelled = true;
      unsubscribe();
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

  const activePlanIdRef = useRef<string | null>(initialActivePlanId);
  const [activePlanId, setActivePlanIdState] = useState<string | null>(initialActivePlanId);
  const setActivePlanId = useCallback((next: string | null) => {
    activePlanIdRef.current = next;
    setActivePlanIdState(next);
  }, []);

  // The in-flight batch of pending decisions, if any — `decide` is what
  // `AssistantToolApprovalGroup`'s Approve/Deny buttons (via `decideToolCall`
  // below) and each approval call's own `TOOL_APPROVAL_TIMEOUT_MS` timeout
  // both funnel through. `answerAskUser` is `AssistantAskUserCard`'s submit
  // path (no timeout — clarifying questions wait until answered or Stop).
  // `denyAll` is `stopChat`'s hook. `null` whenever no round is currently
  // paused awaiting decisions.
  const activeApprovalRef = useRef<{
    decide: (id: string, approved: boolean, trust: boolean) => void;
    answerAskUser: (id: string, answer: AskUserAnswerPayload) => void;
    denyAll: () => void;
  } | null>(null);

  // Which call ids the resume about to run auto-approved via
  // `trustedToolsRef` (as opposed to the user just having clicked Approve)
  // — read by the `listenLlmToolCall` effect below so the resulting block
  // can carry `autoApproved: true` for display. Reassigned right before
  // each `streamLlmChatResume`, since `TOOL_CALL_EVENT`s for that call only
  // start arriving once it's in flight.
  const autoApprovedIdsRef = useRef<Set<string>>(new Set());

  // The proactive history-compaction pass's cached summary + fold boundary
  // (see `src/lib/contextCompaction.ts`) — in-memory only, never persisted,
  // recomputed fresh whenever it's missing/invalid. Safe as a plain
  // per-mount cache because `AssistantConversation` (this hook's caller) is
  // rendered with `key={chatHistory.currentChatId}` in `AssistantPanel.tsx`,
  // so React fully remounts on every chat switch — a stale cache from a
  // different conversation can't survive that. `runTurn` double-checks via
  // `isCacheValid` before every use regardless, so this stays correct even
  // if that remount guarantee is ever weakened later.
  const compactionCacheRef = useRef<CompactionCache | null>(null);

  /** Shows every call in `calls` inline in the transcript as a
   * `"pendingApproval"` block. Resolves once every call has a decision —
   * Approve/Deny (with timeout) for mutating/mode tools, or Submit/Skip
   * (no timeout) for `askUser`. */
  const collectDecisions = useCallback((calls: PendingToolCall[]): Promise<ToolCallDecision[]> => {
    return new Promise((resolve) => {
      type Entry = { approved: boolean; answer?: AskUserAnswerPayload };
      const decided = new Map<string, Entry>();
      const timers = new Map<string, ReturnType<typeof setTimeout>>();
      const approvalDeadlineAt = Date.now() + TOOL_APPROVAL_TIMEOUT_MS;
      const approvalGroupId = crypto.randomUUID();
      const askGroupId = crypto.randomUUID();

      const finalizeIfDone = () => {
        if (decided.size !== calls.length) return;
        activeApprovalRef.current = null;
        resolve(
          calls.map((c) => {
            const entry = decided.get(c.id);
            return {
              id: c.id,
              approved: entry?.approved ?? false,
              ...(entry?.answer ? { answer: entry.answer } : {}),
            };
          }),
        );
      };

      const decide = (id: string, approved: boolean, trust: boolean, answer?: AskUserAnswerPayload) => {
        if (decided.has(id) || !calls.some((c) => c.id === id)) return;
        decided.set(id, { approved, answer });
        const timer = timers.get(id);
        if (timer !== undefined) clearTimeout(timer);
        if (trust && approved) {
          const call = calls.find((c) => c.id === id);
          // `askUser` is never auto-approvable — trust would skip the whole
          // clarifying-question card on later turns.
          if (call && call.name !== "askUser") {
            trustedToolsRef.current.add(call.name);
            void setToolAutoApproved(call.name, true);
          }
        }
        finalizeIfDone();
      };

      activeApprovalRef.current = {
        decide: (id, approved, trust) => decide(id, approved, trust),
        answerAskUser: (id, answer) => decide(id, true, false, answer),
        denyAll: () => calls.forEach((c) => decide(c.id, false, false)),
      };

      const approvalCalls = calls.filter((c) => c.name !== "askUser");
      approvalCalls.forEach((c) => {
        timers.set(
          c.id,
          setTimeout(() => decide(c.id, false, false), TOOL_APPROVAL_TIMEOUT_MS),
        );
      });

      setMessages((prev) =>
        updateLastAssistantBlocks(prev, (blocks) =>
          calls.reduce((acc, c) => {
            const isAsk = c.name === "askUser";
            return appendPendingApprovalBlock(acc, {
              id: c.id,
              name: c.name,
              argumentsJson: c.arguments,
              deadlineAt: isAsk ? undefined : approvalDeadlineAt,
              approvalGroupId: isAsk ? askGroupId : approvalGroupId,
            });
          }, blocks),
        ),
      );

      // One chime per ask-user pause — even if the batch has several
      // `askUser` calls, they share a single card group.
      if (needAnswerSoundEnabledRef.current && calls.some((c) => c.name === "askUser")) {
        playNeedAnswerSound();
      }
    });
  }, []);

  /** Passed down to `AssistantToolApprovalGroup`'s Approve/Deny buttons. */
  const decideToolCall = useCallback((id: string, approved: boolean, trust: boolean) => {
    activeApprovalRef.current?.decide(id, approved, trust);
  }, []);

  /** Passed down to `AssistantAskUserCard`'s Submit button. */
  const answerAskUser = useCallback((id: string, answer: AskUserAnswerPayload) => {
    activeApprovalRef.current?.answerAskUser(id, answer);
  }, []);

  /** Stops the in-flight turn, wherever it currently is: mid-stream,
   * between tool-calling rounds, or waiting on a `"pendingApproval"` /
   * ask-user card. `cancelLlmChat()` sets the backend's cancel flag so the
   * next checkpoint `run_tool_loop` hits resolves `{status: "cancelled"}`
   * instead of continuing — but if a pending card is showing right now,
   * nothing on the backend is actually running to reach one; `denyAll`
   * unblocks `collectDecisions`'s pending promise immediately so
   * `sendMessage`'s `while` loop proceeds straight to `streamLlmChatResume`,
   * which then hits the very first checkpoint before executing any of those
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

  // Live "thinking" deltas from a reasoning-capable model — same shape as
  // the token-delta effect above, just routed into a `reasoning` block
  // instead of `text`. Never fires for a provider/model that doesn't send
  // `reasoning_content`.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listenLlmChatReasoningDelta(({ delta }) => {
      setMessages((prev) => updateLastAssistantBlocks(prev, (blocks) => appendReasoningDeltaToBlocks(blocks, delta)));
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
      if (result?.tool === "planCreated" || result?.tool === "planUpdated") {
        setActivePlanId(result.result.planId);
      } else if (result?.tool === "todoWritten" || result?.tool === "todoUpdated") {
        // The tool's own result already carries the authoritative post-call
        // checklist — reflect it in `TodoProgressWidget` the moment the
        // call settles, rather than waiting for the whole round (which may
        // still have several more tool calls left) to finish streaming.
        setTodos(result.result);
      }
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
  }, [setActivePlanId, setTodos]);

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
  //
  // Declared before `runTurn` (not just before `sendMessage`, its own
  // previous position) so `runTurn`'s `useCallback` body can close over the
  // value — `const` bindings must be declared earlier in the function body
  // to be visible at the point they're read, even though every hook call
  // here still runs top-to-bottom on each render regardless of declaration
  // order.
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
          buildSystemPromptForConversationMode(
            conversationMode,
            accessMode,
            specsRepoInfo,
            toolDefinitions,
            docsRootRelativeToRepo,
          ),
        ) + messages.reduce((sum, m) => sum + estimateTokenCount(chatMessageToPlainText(m)), 0)
      );
    }
    const tail = messages
      .slice(lastUsageIndex + 1)
      .reduce((sum, m) => sum + estimateTokenCount(chatMessageToPlainText(m)), 0);
    return lastUsageTotal + tail;
  }, [messages, accessMode, conversationMode, specsRepoInfo, toolDefinitions, docsRootRelativeToRepo]);

  /** Drives the pending-approval resume loop starting from `initialOutcome`
   * — shared by a live turn (`runTurn`, starting from a fresh
   * `streamLlmChat()` result) and a cold-hydrate on mount (starting from a
   * `PendingApproval` restored from `chat.db`, see the effect below). Each
   * time a round actually pauses (i.e. `collectDecisions` is really shown,
   * not silently auto-approved), persists it via `onTurnPaused` right away
   * — the pending-approval block was just appended to `messages` by
   * `collectDecisions`'s own `setMessages` call, applied in order before
   * this "peek" one — so the turn survives a full app restart even if it's
   * still paused when that happens, not just a panel close. */
  const runPendingLoop = useCallback(
    async (initialOutcome: ChatStreamOutcome): Promise<ChatStreamOutcome> => {
      if (!providerId) return initialOutcome;
      let outcome = initialOutcome;
      while (outcome.status === "pendingApproval") {
        const pending = outcome.value;
        const { history, round, budgetUsed, calls, todos: updatedTodos } = pending;
        setTodos(updatedTodos);
        const risky = calls.filter((c) => c.requiresConfirmation);
        const autoApprovedIds = new Set<string>();
        const needsDecision = risky.filter((c) => {
          // Clarifying questions must always surface — never skip via
          // "always allow" / session trust.
          if (c.name === "askUser") return true;
          if (trustedToolsRef.current.has(c.name)) {
            autoApprovedIds.add(c.id);
            return false;
          }
          return true;
        });

        let decisions: ToolCallDecision[];
        if (needsDecision.length === 0) {
          decisions = risky.map((c) => ({ id: c.id, approved: true }));
        } else {
          const collected = collectDecisions(needsDecision);
          setMessages((prev) => {
            onTurnPaused(prev, todoListRef.current, activePlanIdRef.current, pending);
            return prev;
          });
          decisions = [
            ...(await collected),
            ...risky.filter((c) => autoApprovedIds.has(c.id)).map((c) => ({ id: c.id, approved: true })),
          ];
        }

        autoApprovedIdsRef.current = autoApprovedIds;
        outcome = await streamLlmChatResume(
          providerId,
          history,
          round,
          budgetUsed,
          decisions,
          todoListRef.current,
          activeFilePath,
          conversationMode,
        );
      }
      return outcome;
    },
    [providerId, activeFilePath, conversationMode, collectDecisions, setTodos, onTurnPaused],
  );

  /** Applies a `"done"`/`"cancelled"` outcome's authoritative final text to
   * the in-flight assistant message — shared by `runTurn` and the
   * cold-hydrate effect below, same as `runPendingLoop`. */
  const settleOutcome = useCallback(
    (outcome: ChatStreamOutcome, assistantId: string) => {
      // `runPendingLoop` never returns while `status === "pendingApproval"`
      // (that's its own while-loop's exit condition) — this guard is just
      // to satisfy the type checker across the function-call boundary, not
      // a real runtime possibility.
      if (outcome.status === "pendingApproval") return;
      // Same handling for `"cancelled"` as `"done"` (both carry the same
      // `ChatStreamResult` shape) — the only difference is the extra
      // `cancelled` flag for display and sweeping any pending-approval card
      // `stopChat` auto-denied but that never got its settling event, since
      // `run_tool_loop` returned before reaching it.
      const stoppedByUser = outcome.status === "cancelled";
      const { text, reasoning, usage, todos: finalTodos } = outcome.value;
      setTodos(finalTodos);
      setMessages((prev) =>
        prev.map((m) =>
          m.id === assistantId && m.role === "assistant"
            ? {
                ...m,
                blocks: (() => {
                  const corrected = correctTrailingText(correctTrailingReasoning(m.blocks, reasoning ?? ""), text);
                  return stoppedByUser
                    ? markRunningToolCallsAsInterrupted(corrected, "Остановлено пользователем")
                    : corrected;
                })(),
                streaming: false,
                usage: usage ?? undefined,
                cancelled: stoppedByUser,
              }
            : m,
        ),
      );
      if (!stoppedByUser && taskDoneSoundEnabledRef.current) {
        playTaskDoneSound();
      }
    },
    [setTodos],
  );

  /** Marks the in-flight assistant message as failed — shared by `runTurn`
   * and the cold-hydrate effect below, same as `runPendingLoop`. */
  const settleError = useCallback((e: unknown, assistantId: string) => {
    const message = e instanceof Error ? e.message : String(e);
    // Best-effort: drives the "Сжать историю и повторить" retry action
    // (`retryWithCompaction`) rather than just showing raw error text — see
    // `isContextLengthError`'s doc comment for why this can't be a reliable
    // classification, only a heuristic.
    const contextLengthExceeded = isContextLengthError(message);
    setMessages((prev) =>
      prev.map((m) =>
        m.id === assistantId && m.role === "assistant"
          ? {
              ...m,
              blocks: markRunningToolCallsAsInterrupted(m.blocks),
              streaming: false,
              failed: true,
              errorMessage: message,
              contextLengthExceeded,
            }
          : m,
      ),
    );
  }, []);

  /** Runs one full turn: an optional proactive history-compaction pass,
   * building `wireMessages` from `priorTurns` (replaying the cached
   * compaction summary plus whatever's after its boundary, instead of the
   * full history, when a valid cache exists), the `streamLlmChat`/
   * `streamLlmChatResume` pending-approval loop, and the success/error/
   * settle handling — everything `sendMessage` used to do inline. Shared by
   * `sendMessage` (`opts.aggressiveCompaction: false`) and
   * `retryWithCompaction` (`true`, and a smaller keep-tail) so the two
   * paths can't drift apart. `priorTurns` is passed in rather than read
   * from `messages` directly so a retry can supply a version with the
   * failed turn already removed.
   *
   * `opts.planExecutionStart` marks the canned «Начать» send: planning
   * transcript is dropped from the *wire* (UI chat stays) and a planning-era
   * compaction cache is discarded so GOAL/DECISIONS cannot leak rejected
   * hypotheses into execution. */
  const runTurn = useCallback(
    async (
      userText: string,
      assistantId: string,
      priorTurns: ChatMessage[],
      opts: { aggressiveCompaction: boolean; planExecutionStart?: boolean },
    ) => {
      if (!providerId) return;
      setSending(true);

      const real = realMessages(priorTurns);
      const scoped = sliceMessagesForPlanExecution(real, opts.planExecutionStart === true);

      // A cache surviving from a foreign/removed conversation, or from the
      // planning transcript we just dropped, must never be used — drop it
      // before either deciding whether to compact or building `wireMessages`.
      if (!isCacheValid(compactionCacheRef.current, scoped)) {
        compactionCacheRef.current = null;
      }

      const keepLast = opts.aggressiveCompaction
        ? CONTEXT_COMPACTION_RETRY_KEEP_LAST_MESSAGES
        : CONTEXT_COMPACTION_KEEP_LAST_MESSAGES;

      const scopedTokens =
        estimateTokenCount(
          buildSystemPromptForConversationMode(
            conversationMode,
            accessMode,
            specsRepoInfo,
            toolDefinitions,
            docsRootRelativeToRepo,
          ),
        ) + scoped.reduce((sum, m) => sum + estimateTokenCount(chatMessageToPlainText(m)), 0);

      if (opts.aggressiveCompaction || shouldCompact(scopedTokens, contextLimit, scoped)) {
        try {
          const compaction = planCompaction(scoped, compactionCacheRef.current, keepLast, activeFilePath);
          if (compaction) {
            const excerpt = compaction.toSummarize.map(describeMessageForCompaction).join("\n\n");
            const prompt = buildHistoryCompactionPrompt(compactionCacheRef.current?.summaryText ?? null, excerpt);
            const response = await llmChatOnce(providerId, [{ role: "user", content: prompt, toolCallId: null }]);
            const summaryText = response.content?.trim();
            if (summaryText) {
              const fromOrdinal = real.findIndex((m) => m.id === compaction.toSummarize[0]!.id) + 1;
              const toOrdinal = real.findIndex((m) => m.id === compaction.toSummarize[compaction.toSummarize.length - 1]!.id) + 1;
              compactionCacheRef.current = { summaryText, boundaryMessageId: compaction.newBoundaryId };
              const noticeMsg: ChatMessage = {
                id: crypto.randomUUID(),
                role: "assistant",
                blocks: [
                  { type: "text", id: crypto.randomUUID(), content: formatCompactionNoticeText(fromOrdinal, toOrdinal) },
                ],
                streaming: false,
                isCompactionNotice: true,
              };
              setMessages((prev) => [...prev, noticeMsg]);
            }
          }
        } catch (e) {
          // Never block the user's actual message over a failed
          // summarization call — if history really is too large to send as
          // it stands, `isContextLengthError`'s reactive net below (via a
          // later `retryWithCompaction`) is the backstop, not this pass.
          console.error("Не удалось сжать историю чата", e);
        }
      }

      let wireTail = scoped;
      if (compactionCacheRef.current) {
        const boundaryIndex = scoped.findIndex((m) => m.id === compactionCacheRef.current!.boundaryMessageId);
        if (boundaryIndex === -1) {
          // Stale cache slipped past the earlier `isCacheValid` check
          // somehow (e.g. the boundary message was removed mid-turn) —
          // fall back to the (already scoped) history rather than inject a
          // summary that no longer corresponds to anything being sent.
          compactionCacheRef.current = null;
        } else {
          wireTail = scoped.slice(boundaryIndex + 1);
        }
      }

      // Placed as its own message right before the new user turn (not just
      // folded into the system prompt above) so a mode switch is impossible
      // for the model to skim past — see `buildAccessModeChangeNotice`'s
      // doc comment for why the system prompt's own mode line isn't
      // reliably enough on its own once a conversation has some history.
      const modeChanged = lastSentModeRef.current !== null && lastSentModeRef.current !== accessMode;
      lastSentModeRef.current = accessMode;

      // Same "own message, not just a rebuilt system-prompt line" treatment
      // for a conversation-mode switch — see `buildModeChangeNotice`.
      const conversationModeChanged =
        lastSentConversationModeRef.current !== null &&
        lastSentConversationModeRef.current !== conversationMode;
      lastSentConversationModeRef.current = conversationMode;

      const todoBlock = buildTodoContextBlock(todoListRef.current);
      // Editor tabs store docs-relative paths; the model sees access-mode
      // paths (repo-relative in Full-repo). Backend `activeFilePath` stays
      // docs-relative for FileId conversion.
      const activeFileForPrompt =
        accessMode === "fullRepo" && docsRootRelativeToRepo && activeFilePath
          ? `${docsRootRelativeToRepo}/${activeFilePath}`
          : activeFilePath;
      const activeFileBlock = buildActiveFileContextBlock(activeFileForPrompt);

      const planId = activePlanIdRef.current;
      let planRecord: PlanRecord | null = null;
      if (planId && conversationMode === "agent") {
        try {
          planRecord = await planGet(planId);
        } catch (e) {
          console.error("Не удалось прочитать активный план", e);
          planRecord = null;
        }
      }
      const activePlanBlock = buildActivePlanContextBlock(planId, planRecord);

      let memoryBlock: string | null = null;
      try {
        memoryBlock = buildMemoryContextBlock(await getMemoryWake());
      } catch (e) {
        // "No project open" is expected and not worth logging; anything
        // else (a corrupt store, an I/O error, a bad hand-edited config
        // knob) would otherwise degrade memory context for the rest of the
        // session with zero trace — see the compaction catch above for the
        // same reasoning.
        const message = e instanceof Error ? e.message : String(e);
        if (!message.includes("no project open")) {
          console.error("Не удалось прочитать память ассистента", e);
        }
        memoryBlock = null;
      }

      const wireMessages: LlmMessage[] = [
        {
          role: "system",
          content: buildSystemPromptForConversationMode(
            conversationMode,
            accessMode,
            specsRepoInfo,
            toolDefinitions,
            docsRootRelativeToRepo,
          ),
          toolCallId: null,
        },
        ...(compactionCacheRef.current
          ? [
              {
                role: "system" as const,
                content: buildCompactionSummaryBlock(compactionCacheRef.current.summaryText),
                toolCallId: null,
              },
            ]
          : []),
        ...wireTail.map((m): LlmMessage => ({ role: m.role, content: chatMessageToPlainText(m), toolCallId: null })),
        ...(modeChanged
          ? [
              {
                role: "system" as const,
                content: buildAccessModeChangeNotice(accessMode, docsRootRelativeToRepo),
                toolCallId: null,
              },
            ]
          : []),
        ...(conversationModeChanged
          ? [{ role: "system" as const, content: buildModeChangeNotice(conversationMode), toolCallId: null }]
          : []),
        ...(memoryBlock ? [{ role: "system" as const, content: memoryBlock, toolCallId: null }] : []),
        ...(activeFileBlock ? [{ role: "system" as const, content: activeFileBlock, toolCallId: null }] : []),
        ...(todoBlock ? [{ role: "system" as const, content: todoBlock, toolCallId: null }] : []),
        ...(activePlanBlock ? [{ role: "system" as const, content: activePlanBlock, toolCallId: null }] : []),
        { role: "user", content: userText, toolCallId: null },
      ];

      try {
        // Most turns resolve in one round trip. A round that hits a call
        // needing confirmation (`writeFile`/`requestFullRepoAccess`) comes
        // back as `pendingApproval` instead — nothing in it executed yet —
        // and `runPendingLoop` collects a decision for each call (skipping
        // the card entirely for tool names already trusted, whether from
        // this chat or persisted from an earlier one on this project)
        // before resuming, potentially several times if later rounds pause
        // again. Authoritative full text of the *final* round corrects only
        // the trailing text block — see `correctTrailingText`'s doc comment
        // for why that's always the right (and only) block it can apply to.
        const outcome = await runPendingLoop(
          await streamLlmChat(providerId, wireMessages, todoListRef.current, activeFilePath, conversationMode),
        );
        settleOutcome(outcome, assistantId);
      } catch (e) {
        settleError(e, assistantId);
      } finally {
        setSending(false);
        // Reads the true final state for this turn via a functional-update
        // "peek" — the `try`/`catch` block's own `setMessages` call and
        // this one happen synchronously in the same tick (no `await`
        // between them), so React's batching applies them to the update
        // queue in order; this updater sees exactly what the turn ended
        // with, covering both the success and error paths in one place.
        setMessages((prev) => {
          onTurnSettled(prev, todoListRef.current, activePlanIdRef.current);
          return prev;
        });
      }
    },
    [
      providerId,
      contextLimit,
      accessMode,
      conversationMode,
      specsRepoInfo,
      toolDefinitions,
      docsRootRelativeToRepo,
      runPendingLoop,
      settleOutcome,
      settleError,
      onTurnSettled,
      activeFilePath,
    ],
  );

  // One-shot: if this chat was loaded with an unresolved pause — the app
  // was fully restarted (or crashed) while paused on a tool-approval/
  // `askUser` card, not just the panel being hidden within one running
  // session (`RightDock` never unmounts this hook for that case) — replay
  // the exact same `collectDecisions`/`streamLlmChatResume` flow a live
  // pause would have used. The card itself needs no separate reconstruction
  // — it's already part of `initialMessages` (persisted alongside
  // `pending_resume`), so this only needs to re-arm `activeApprovalRef` so
  // its buttons work again. Guarded on `initialMessages`' own trailing
  // entry (not live `messages` state, which would make this effect refire
  // on every later render) still being the paused turn's open assistant
  // message — a stale/corrupted row must never resume blind.
  const coldResumedRef = useRef(false);
  useEffect(() => {
    if (!initialPendingResume || coldResumedRef.current || !providerId) return;
    const last = initialMessages[initialMessages.length - 1];
    if (!last || last.role !== "assistant" || !last.streaming) return;
    coldResumedRef.current = true;
    const assistantId = last.id;
    setSending(true);
    void (async () => {
      try {
        const outcome = await runPendingLoop({ status: "pendingApproval", value: initialPendingResume });
        settleOutcome(outcome, assistantId);
      } catch (e) {
        settleError(e, assistantId);
      } finally {
        setSending(false);
        setMessages((prev) => {
          onTurnSettled(prev, todoListRef.current, activePlanIdRef.current);
          return prev;
        });
      }
    })();
  }, [initialPendingResume, initialMessages, providerId, runPendingLoop, settleOutcome, settleError, onTurnSettled]);

  const sendMessage = useCallback(
    async (text: string, opts?: { planExecutionStart?: boolean }) => {
      const trimmed = text.trim();
      if (!providerId || sending || !trimmed) return;

      const priorTurns = messages;
      const userMsg: ChatMessage = {
        id: crypto.randomUUID(),
        role: "user",
        content: trimmed,
        ...(opts?.planExecutionStart ? { isPlanExecutionStart: true } : {}),
      };
      const assistantId = crypto.randomUUID();
      setMessages((prev) => [
        ...prev,
        userMsg,
        { id: assistantId, role: "assistant", blocks: [], streaming: true },
      ]);
      await runTurn(trimmed, assistantId, priorTurns, {
        aggressiveCompaction: false,
        planExecutionStart: opts?.planExecutionStart === true,
      });
    },
    [providerId, sending, messages, runTurn],
  );

  /** The "Сжать историю и повторить" action on a failed message whose
   * `contextLengthExceeded` flag is set (see the catch block above). Drops
   * the failed bubble, replays the same user text through `runTurn` with
   * `aggressiveCompaction: true` (a smaller keep-tail, and a compaction pass
   * that runs regardless of `shouldCompact`'s ratio check). This mitigates
   * not just a normal cross-turn overflow but also the case where a single
   * turn's own tool-calling loop (`run_tool_loop`, backend-side) grew
   * `history` past the limit internally before erroring — the retry
   * rebuilds `wireMessages` from this hook's own `messages` array, which
   * never contained that transient, backend-only blowup, so it's inherently
   * smaller regardless of what caused the original failure. */
  const retryWithCompaction = useCallback(
    (assistantMessageId: string) => {
      if (!providerId || sending) return;
      const failedIndex = messages.findIndex((m) => m.id === assistantMessageId);
      const failedMsg = failedIndex === -1 ? undefined : messages[failedIndex];
      if (!failedMsg || failedMsg.role !== "assistant" || !failedMsg.failed) return;
      const userMsgToRetry = messages[failedIndex - 1];
      if (!userMsgToRetry || userMsgToRetry.role !== "user") return;

      const priorTurns = messages.slice(0, failedIndex - 1);
      const newAssistantId = crypto.randomUUID();
      setMessages((prev) => [
        ...prev.filter((m) => m.id !== assistantMessageId),
        { id: newAssistantId, role: "assistant", blocks: [], streaming: true },
      ]);
      void runTurn(userMsgToRetry.content, newAssistantId, priorTurns, {
        aggressiveCompaction: true,
        planExecutionStart: userMsgToRetry.isPlanExecutionStart === true,
      });
    },
    [providerId, sending, messages, runTurn],
  );

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
    onTurnSettled(messages, next, activePlanIdRef.current);
  }, [messages, onTurnSettled, setTodos]);

  return {
    messages,
    sending,
    sendMessage,
    retryWithCompaction,
    contextTokens,
    todos,
    clearTodos,
    activePlanId,
    setActivePlanId,
    systemPrompt: buildSystemPromptForConversationMode(
      conversationMode,
      accessMode,
      specsRepoInfo,
      toolDefinitions,
      docsRootRelativeToRepo,
    ),
    decideToolCall,
    answerAskUser,
    stopChat,
  };
}
