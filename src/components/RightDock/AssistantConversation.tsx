import { useCallback, useEffect, useMemo, useRef, useState, type WheelEvent } from "react";
import { AlertCircle, ArrowDown, ChevronUp, Clock3, FileText, FolderGit2, Send, Sparkles, Square, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useLlmChat } from "../../hooks/useLlmChat";
import { formatElapsedDuration } from "../../hooks/useElapsedSeconds";
import {
  AUTO_MODEL_LABEL,
  AUTO_MODEL_VALUE,
  CHAT_INPUT_ROWS,
  CHAT_MODEL_CATALOG_EMPTY_HINT,
  CONTEXT_NEAR_LIMIT_RATIO,
  PLAN_EXECUTION_START_TEXT,
} from "../../lib/assistantConfig";
import {
  ASSISTANT_SUGGESTIONS,
  buildSuggestionContext,
  needsAccessUpgrade,
  needsSuggestionForm,
  prefillValues,
  renderSuggestionText,
  suggestionsForMode,
  visibleSuggestions,
} from "../../lib/assistantSuggestions";
import type { AssistantSuggestion } from "../../lib/assistantSuggestions";
import type { AiAccessMode, ConversationMode, LlmToolDefinition, Task } from "../../lib/aiTools";
import { groupBlocksForRender, lastBlockShowsLiveProgress, openStreamingBlockIds, searchIsDegraded, type ChatMessage } from "../../lib/chatBlocks";
import { noteLlmChat } from "../../lib/llm";
import type { LlmProviderConfig, PendingApproval, ResolvedLlmProvider } from "../../lib/llm";
import type { SpecsRepoInfo } from "../../lib/openapi";
import type { UpdatedReference } from "../../lib/project";
import { AssistantMarkdown } from "./AssistantMarkdown";
import { AssistantArtifactCard } from "./AssistantArtifactCard";
import { AssistantAskUserCard } from "./AssistantAskUserCard";
import { AssistantCompactionNotice } from "./AssistantCompactionNotice";
import { AssistantReasoningBlock, AssistantThinkingIndicator } from "./AssistantReasoningBlock";
import { AssistantSteerBlock } from "./AssistantSteerBlock";
import { AssistantSuggestionChip } from "./AssistantSuggestionChip";
import { AssistantSuggestionModal } from "./AssistantSuggestionModal";
import { AssistantToolApprovalGroup } from "./AssistantToolApprovalGroup";
import { AssistantToolCallBlock } from "./AssistantToolCallBlock";
import { AssistantUserMessage } from "./AssistantUserMessage";
import { AssistantPlanCard, isPlanToolBlock } from "./AssistantPlanCard";
import { openArtifactTab } from "../../lib/artifactTabs";
import { AssistantTicketCard, isTicketToolBlock } from "./AssistantTicketCard";
import { AssistantVisualCard, isVisualToolBlock } from "./AssistantVisualCard";
import { openVisualTab } from "../../lib/visuals";
import { TodoProgressWidget } from "./TodoProgressWidget";
import { PlanProgressWidget } from "./PlanProgressWidget";
import { trackMetric } from "../../lib/metrics";
import { METRICS } from "../../data/metricsCatalog";

const ACCESS_MODE_OPTIONS: { value: AiAccessMode; label: string; Icon: LucideIcon }[] = [
  { value: "docsOnly", label: "Документация", Icon: FileText },
  { value: "fullRepo", label: "Весь репозиторий", Icon: FolderGit2 },
];

/** `title` is the short capability phrase used as a tooltip on the mode chips
 * and in the composer's mode dropdown; `greeting` is the full sentence the
 * empty state shows under «Привет! Я Атлас», which changes with the mode —
 * its opening also says what kind of input the mode expects (a task, a
 * question), so the two halves of the placeholder don't repeat each other. */
const CHAT_MODE_OPTIONS: {
  value: ConversationMode;
  label: string;
  title: string;
  greeting: string;
}[] = [
  {
    value: "agent",
    label: "Агент",
    title: "смогу исследовать репозиторий и вносить изменения в документацию",
    greeting: "Расскажите, что нужно сделать — изучу проект и внесу правки в документацию.",
  },
  {
    value: "plan",
    label: "План",
    title: "смогу составлять план будущих работ без правок в документацию",
    greeting: "Опишите задачу — изучу проект и составлю план работ, не меняя файлы.",
  },
  {
    value: "question",
    label: "Вопрос",
    title: "смогу отвечать на точечные вопросы не внося изменений в документацию",
    greeting: "Спросите о проекте — разберусь и отвечу, не меняя файлы.",
  },
];

/** Ids of already-settled `requestModeSwitch` tool calls in a transcript —
 * used to seed `handledModeSwitchIdsRef` so restoring / remounting a chat
 * does not re-apply a historical mode onto the session-scoped picker. */
function collectSettledModeSwitchIds(messages: ChatMessage[]): Set<string> {
  const ids = new Set<string>();
  for (const message of messages) {
    if (message.role !== "assistant") continue;
    for (const block of message.blocks) {
      if (block.type === "toolCall" && block.name === "requestModeSwitch" && block.status === "done") {
        ids.add(block.id);
      }
    }
  }
  return ids;
}

/** Docs-only vs full-repo — compact icon toggle in the model bar. */
function AccessModeToggle({
  mode,
  onChange,
  disabled,
}: {
  mode: AiAccessMode;
  onChange: (mode: AiAccessMode) => void;
  disabled: boolean;
}) {
  return (
    <div className="assistant-access-toggle" role="radiogroup" aria-label="Область доступа AI">
      {ACCESS_MODE_OPTIONS.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={mode === option.value}
          aria-label={option.label}
          title={option.label}
          className={`assistant-access-btn${mode === option.value ? " active" : ""}`}
          disabled={disabled}
          onClick={() => onChange(option.value)}
        >
          <option.Icon size={12} strokeWidth={1.75} aria-hidden />
        </button>
      ))}
    </div>
  );
}

/** Chat-composer mode picker — controlled by the parent (`conversationMode`/
 * `onConversationModeChange` on `AssistantConversationProps`), so both a
 * manual click here and an approved `requestModeSwitch` tool call (see the
 * settled-block watcher below) converge on the exact same state. */
function ChatModeSelect({
  mode,
  onChange,
  disabled,
}: {
  mode: ConversationMode;
  onChange: (mode: ConversationMode) => void;
  disabled: boolean;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  const active = CHAT_MODE_OPTIONS.find((o) => o.value === mode);

  return (
    <div className="assistant-mode-select" ref={ref}>
      <button
        type="button"
        className={`assistant-mode-trigger${open ? " is-open" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        title={active?.title ?? "Режим ассистента"}
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
      >
        <span>{active?.label ?? ""}</span>
        <ChevronUp className="assistant-mode-chevron" size={12} aria-hidden />
      </button>
      {open ? (
        <div className="assistant-mode-menu" role="listbox">
          {CHAT_MODE_OPTIONS.map((option) => (
            <button
              key={option.value}
              type="button"
              role="option"
              aria-selected={mode === option.value}
              title={option.title}
              className={`assistant-mode-option${mode === option.value ? " is-active" : ""}`}
              onClick={() => {
                onChange(option.value);
                setOpen(false);
              }}
            >
              <span>{option.label}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function trimTrailingZero(n: number): string {
  return Number.isInteger(n) ? String(n) : n.toFixed(1);
}

/** Форматирует вложение для отправки модели: строка с путём файла (если
 * есть) плюс построчная цитата `> `. Вложения не попадают в черновик как
 * текст (см. `ChatAttachment`) — это форматирование применяется только к
 * итоговому сообщению в момент отправки. */
function formatSelectionForChat(text: string, filePath: string | null): string {
  const pathLine = filePath ? `Из \`${filePath}\`:\n` : "";
  const quoted = text
    .split("\n")
    .map((line) => (line.trim() ? `> ${line}` : ">"))
    .join("\n");
  return `${pathLine}${quoted}`;
}

/** «Добавить в чат» из редактора — выделенный фрагмент, показанный в поле
 * ввода как компактный чип (а не как сырой цитированный текст, который бы
 * захламлял черновик), но по-прежнему уходящий модели целиком при отправке. */
type ChatAttachment = {
  id: number;
  text: string;
  filePath: string | null;
};

/** Подпись чипа: имя файла (или «Фрагмент» для вставок без пути) плюс
 * количество строк/символов — ровно то, что нужно узнать вложение, не
 * разворачивая его. */
function attachmentLabel(attachment: ChatAttachment): string {
  const lines = attachment.text.split("\n").length;
  const sizeLabel = lines > 1 ? `${lines} строк` : `${attachment.text.length} симв.`;
  const name = attachment.filePath?.split(/[/\\]/).pop();
  return name ? `${name} · ${sizeLabel}` : `Фрагмент · ${sizeLabel}`;
}

/** Above this count, chips give way to a single «Все (N)» toggle instead of
 * wrapping onto more rows — `.assistant-chat-input-row` is `flex: none`, so
 * an unbounded chip cloud would keep growing at the expense of the
 * transcript above it until the model's reply scrolled out of view. */
const ATTACHMENTS_INLINE_LIMIT = 2;

/** One attachment, rendered either as an inline pill (`variant="chip"`, the
 * collapsed row) or as a full-width row (`variant="row"`, inside the
 * expanded list) — same content either way, only the wrapping class
 * differs. */
function AttachmentChip({
  attachment,
  variant,
  onRemove,
}: {
  attachment: ChatAttachment;
  variant: "chip" | "row";
  onRemove: () => void;
}) {
  const label = attachmentLabel(attachment);
  return (
    <span
      className={variant === "chip" ? "assistant-chat-attachment-chip" : "assistant-chat-attachment-row"}
      role="listitem"
      title={attachment.text}
    >
      <FileText size={12} strokeWidth={1.75} aria-hidden />
      <span className="assistant-chat-attachment-label">{label}</span>
      <button
        type="button"
        className="assistant-chat-attachment-remove"
        aria-label={`Убрать вложение: ${label}`}
        onClick={onRemove}
      >
        <X size={10} strokeWidth={2} aria-hidden />
      </button>
    </span>
  );
}

function formatTokenCount(n: number): string {
  if (n >= 1_000_000) return `${trimTrailingZero(n / 1_000_000)}M`;
  if (n >= 1_000) return `${trimTrailingZero(n / 1_000)}K`;
  return String(n);
}

/** The ring's tooltip. Two numbers, named, because they answer different
 *  questions and routinely disagree: the ring itself estimates what the
 *  *next* request will carry (the same basis the compaction trigger uses,
 *  so the ring predicts when compaction fires), while the provider's own
 *  count is of a request that already went out — for a turn that read
 *  several files that one is far larger, since a settled turn is replayed
 *  as prose plus a path ledger rather than its tool payloads. Naming them
 *  is what stops the difference from reading as silent compaction. */
function contextUsageTitle(
  contextTokens: number,
  contextLimit: number,
  lastRequestTokens: number | null,
  sending: boolean,
): string {
  const ru = (n: number) => n.toLocaleString("ru-RU");
  const head = sending
    ? `Запрос сейчас: ~${ru(contextTokens)} из ${ru(contextLimit)} токенов`
    : `Следующий запрос: ~${ru(contextTokens)} из ${ru(contextLimit)} токенов`;
  return lastRequestTokens === null
    ? head
    : `${head}\nПоследний отправленный запрос: ${ru(lastRequestTokens)}`;
}

// Geometry for the context-usage ring (see `.assistant-context-ring` in
// AssistantPanel.css) — an SVG circle's stroke-dasharray/-dashoffset trick,
// not a library: the fill circle's dash length equals the full
// circumference and its offset shrinks as usage grows, so the visible arc
// sweeps clockwise from 12 o'clock (the ring itself is rotated -90deg in
// CSS to make that the start point).
const CONTEXT_RING_RADIUS = 8;
const CONTEXT_RING_CIRCUMFERENCE = 2 * Math.PI * CONTEXT_RING_RADIUS;

/** Shared, never-mutated stand-in for a finished message's (empty) set of
 * live blocks — avoids allocating a `Set` per message on every render. */
const EMPTY_LIVE_BLOCK_IDS: ReadonlySet<string> = new Set<string>();

type AssistantConversationProps = {
  /** The chat this instance owns — the parent (`AssistantPanel`) is
   * expected to remount this component (via `key={chatId}`) whenever the
   * active chat changes, which is what resets every bit of per-conversation
   * state below (including `useLlmChat`'s own internal refs — trust set,
   * in-flight approval timers) cleanly, without a manual reset effect that
   * could race an in-flight turn. */
  chatId: string | null;
  initialMessages: ChatMessage[];
  /** The chat's persisted todo checklist, seeding `useLlmChat`'s
   * `todoListRef` — same remount-driven reset as `initialMessages`. */
  initialTodos: Task[];
  initialActivePlanId: string | null;
  /** Set when this chat was last saved mid-turn, paused awaiting a
   * tool-approval/`askUser` decision never resolved before the app closed —
   * lets `useLlmChat` resume the turn (via `streamLlmChatResume`) after a
   * full app restart, not just a same-session panel close. */
  initialPendingResume: PendingApproval | null;
  onTurnSettled: (messages: ChatMessage[], todos: Task[], activePlanId: string | null) => void;
  /** Fires the moment a round pauses awaiting a tool-approval/`askUser`
   * decision, not just once the whole turn eventually settles like
   * `onTurnSettled` — lets the parent persist enough to resume the pause
   * itself even if the app closes before the turn ever finishes. */
  onTurnPaused: (messages: ChatMessage[], todos: Task[], activePlanId: string | null, pendingResume: PendingApproval) => void;
  /** Bubbles `useLlmChat`'s `sending` up to the parent, which uses it to
   * disable chat-switching/new-chat while a turn (including a
   * `pendingApproval` pause) is in flight — see `AssistantPanel.tsx`'s own
   * doc comment on why that gate is load-bearing, not just UX polish. */
  onSendingChange: (sending: boolean) => void;
  providerId: string | null;
  accessMode: AiAccessMode;
  accessModeBusy: boolean;
  onAccessModeChange: (mode: AiAccessMode) => void;
  /** The chat composer's Агент/План/Вопрос mode — a session-scoped setting
   * owned by `AssistantPanel` (not reset on chat switch, not persisted per
   * chat, mirroring `accessMode`'s own lifetime), lifted here so both a
   * manual `ChatModeSelect` click and an approved `requestModeSwitch` tool
   * call converge on the same state. */
  conversationMode: ConversationMode;
  onConversationModeChange: (mode: ConversationMode) => void;
  specsRepoInfo: SpecsRepoInfo | null;
  toolDefinitions: LlmToolDefinition[];
  /** The documentation root's path relative to the repository root (e.g.
   * `"src/docs/asciidoc"`), or `null` when the distinction doesn't matter —
   * see `docsRootRelativeToRepo` in `lib/paths.ts`. Forwarded into
   * `useLlmChat` so the system prompt states the real Full-repo-mode path
   * prefix instead of a generic illustrative example. */
  docsRootRelativeToRepo: string | null;
  docsRoot: string;
  repoRoot: string;
  /** The currently-open editor tab's path (docs-root-relative, same
   * convention `readProjectFile`/`writeProjectFile` already use), or `null`
   * when nothing's open — forwarded into `useLlmChat` so `SemanticSearch`
   * can boost results related to whatever the user is looking at. */
  activeFilePath: string | null;
  /** Whether the repository has uncommitted changes to *tracked* files
   * (`hasTrackedGitChanges` on App's always-live `useGitPanel` status).
   * Only feeds `appliesTo` on the suggestion chips — untracked files are
   * deliberately excluded, since `gitDiff` (what the «Проверить мои правки»
   * suggestion relies on) can't show them either. */
  hasUncommittedChanges: boolean;
  /** Fires once a `writeFile`/`editFile`/`deleteFile`/`createDirectory`/
   * `deleteDirectory` tool call actually lands on disk (its block settles
   * to `"done"`) — `tool`/`path` mirror the settled call so the caller can
   * both refresh the sidebar tree and reconcile any open editor tab for
   * that path (a stale tab left un-reloaded after an out-of-band AI edit
   * would otherwise autosave its old content right back over the change —
   * see `App.tsx`'s handler). */
  onFileWritten: (info: { tool: string; path: string }) => void;
  /** Fires once a `move` tool call actually lands on disk — carries both
   * `from` and `to` (plus the cascaded reference-rewrite report), unlike
   * `onFileWritten`'s single `path`, so the caller can remap an open editor
   * tab from the old path to the new one instead of just reloading it. */
  onFileMoved: (info: { from: string; to: string; updatedFiles: UpdatedReference[] }) => void;
  refreshAccessMode: () => Promise<void>;
  activeProvider: ResolvedLlmProvider | null;
  updateProviderConfig: (providerId: string, patch: Partial<Omit<LlmProviderConfig, "id">>) => Promise<void>;
  /** Re-fetch provider config from the backend — called when the model
   * picker opens so the catalog reflects Settings changes made since the
   * panel was last visible. */
  refreshLlmSetup: () => Promise<void>;
  /** Settings-tab toggle (`LlmSettings.followUpSuggestionsDisabled`,
   * inverted) — gates only the follow-up chip bar shown above the
   * transcript once a branch is picked. Never affects the empty-state
   * chip row, which always renders regardless of this flag. */
  followUpSuggestionsEnabled: boolean;
  /** Settings-tab toggle — play a chime when a turn finishes successfully. */
  taskDoneSoundEnabled: boolean;
  /** Settings-tab toggle — play a chime when an `askUser` card appears. */
  needAnswerSoundEnabled: boolean;
  /** «Добавить в чат» из редактора — запрос на вставку выделенного фрагмента
   * в черновик ввода. Обрабатывается по `id` (тот же паттерн «запрос с
   * счётчиком», что и `insertRequest` в Editor.tsx), чтобы повторный клик по
   * тому же выделению не проглатывался. */
  chatInsertRequest: {
    id: number;
    text: string;
    filePath: string | null;
  } | null;
  /** Вызывается сразу после вставки `chatInsertRequest` в черновик — сигнал
   * родителю (App) очистить запрос. Без этого запрос пережил бы этот
   * remount-able компонент (он монтируется заново при смене чата) и был бы
   * вставлен повторно в свежий черновик следующего чата. */
  onChatInsertHandled?: () => void;
  /** Editor context action — canned prompt to send immediately (not a draft). */
  assistantSendRequest?: {
    id: number;
    text: string;
    conversationMode?: ConversationMode;
  } | null;
  /** Cleared in App after this component consumes `assistantSendRequest`. */
  onAssistantSendHandled?: () => void;
  /** Editor context action — canned prompt inserted into the composer. */
  assistantDraftRequest?: {
    id: number;
    text: string;
    conversationMode?: ConversationMode;
  } | null;
  onAssistantDraftHandled?: () => void;
};

/** The actual per-conversation surface: message transcript, model picker,
 * context-usage ring, and the input row — everything `AssistantPanel.tsx`
 * used to render below its access-mode toggle before chat history split it
 * into an outer shell (owns which chat is active) plus this remountable
 * body (owns one conversation). Always rendered with an LLM provider ready
 * — the parent gates that, so unlike the old single-file component this
 * one doesn't need its own `llmReady` checks. */
export function AssistantConversation({
  chatId,
  initialMessages,
  initialTodos,
  initialActivePlanId,
  initialPendingResume,
  onTurnSettled,
  onTurnPaused,
  onSendingChange,
  providerId,
  accessMode,
  accessModeBusy,
  onAccessModeChange,
  conversationMode,
  onConversationModeChange,
  specsRepoInfo,
  toolDefinitions,
  docsRootRelativeToRepo,
  docsRoot,
  repoRoot,
  activeFilePath,
  hasUncommittedChanges,
  onFileWritten,
  onFileMoved,
  refreshAccessMode,
  activeProvider,
  updateProviderConfig,
  refreshLlmSetup,
  followUpSuggestionsEnabled,
  taskDoneSoundEnabled,
  needAnswerSoundEnabled,
  chatInsertRequest,
  onChatInsertHandled,
  assistantSendRequest,
  onAssistantSendHandled,
  assistantDraftRequest,
  onAssistantDraftHandled,
}: AssistantConversationProps) {
  const contextLimit = activeProvider?.limit?.context ?? null;

  const {
    messages,
    sending,
    pendingSteers,
    sendMessage,
    steerChat,
    retryWithCompaction,
    stopChat,
    contextTokens,
    lastRequestTokens,
    decideToolCall,
    answerAskUser,
    answerArtifact,
    todos,
    clearTodos,
    activePlanId,
    setActivePlanId,
  } = useLlmChat(
    providerId,
    contextLimit,
    accessMode,
    conversationMode,
    specsRepoInfo,
    toolDefinitions,
    docsRootRelativeToRepo,
    initialMessages,
    initialTodos,
    initialActivePlanId,
    initialPendingResume,
    onTurnSettled,
    onTurnPaused,
    activeFilePath,
    taskDoneSoundEnabled,
    needAnswerSoundEnabled,
  );

  const pendingStartPlanRef = useRef(false);
  const pendingAssistantSendRef = useRef<{
    text: string;
    conversationMode: ConversationMode;
  } | null>(null);

  const runAssistantSend = useCallback(
    (text: string, targetMode: ConversationMode = "agent") => {
      if (sending || !text.trim()) return;
      if (conversationMode === targetMode) {
        void sendMessage(text);
        return;
      }
      pendingAssistantSendRef.current = { text, conversationMode: targetMode };
      onConversationModeChange(targetMode);
    },
    [conversationMode, sending, sendMessage, onConversationModeChange],
  );

  useEffect(() => {
    const pending = pendingAssistantSendRef.current;
    if (!pending || conversationMode !== pending.conversationMode || sending) return;
    pendingAssistantSendRef.current = null;
    void sendMessage(pending.text);
  }, [conversationMode, sending, sendMessage]);

  const startPlan = (planId: string) => {
    setActivePlanId(planId);
    if (conversationMode === "agent") {
      // Already in Agent mode — the mode-change effect below never fires
      // (onConversationModeChange("agent") is a same-value no-op), so send
      // directly instead of leaving pendingStartPlanRef stuck true.
      if (!sending) void sendMessage(PLAN_EXECUTION_START_TEXT, { planExecutionStart: true });
      return;
    }
    pendingStartPlanRef.current = true;
    onConversationModeChange("agent");
  };

  useEffect(() => {
    if (conversationMode !== "agent" || !pendingStartPlanRef.current || sending) return;
    pendingStartPlanRef.current = false;
    void sendMessage(PLAN_EXECUTION_START_TEXT, { planExecutionStart: true });
  }, [conversationMode, sending, sendMessage]);

  useEffect(() => {
    const onStart = (event: Event) => {
      const planId = (event as CustomEvent<{ planId?: string }>).detail?.planId;
      if (!planId || sending) return;
      startPlan(planId);
    };
    window.addEventListener("atlas-start-plan", onStart);
    return () => window.removeEventListener("atlas-start-plan", onStart);
  }, [sending, setActivePlanId, onConversationModeChange]);


  useEffect(() => {
    onSendingChange(sending);
  }, [sending, onSendingChange]);

  // Two side effects that only become known once a tool-call block settles
  // to "done" (the real `TOOL_RESULT_EVENT`, after the backend has actually
  // applied the change) — reacting to the transcript here, rather than to
  // the approval decision itself, avoids racing the resume call that does
  // the actual work. Each `handledIdsRef` keeps its effect idempotent
  // across re-renders, since a block stays in `messages` forever once
  // settled: a granted `requestFullRepoAccess` needs `useAiAccessMode`'s
  // local state refreshed so the mode toggle reflects it immediately, and a
  // successful `writeFile`/`createDirectory` needs `App.tsx`'s docs-tree
  // state refreshed (via `onFileWritten`) so the new/changed file or folder
  // actually shows up in the sidebar — nothing else invalidates that tree
  // on the assistant's behalf. Restoring a chat whose last message already
  // ends in a settled block like this re-fires these once on mount — both
  // are idempotent refreshes, so that's wasted work, not a correctness bug.
  //
  // `requestModeSwitch` is the exception: `ConversationMode` is session-
  // scoped (not per-chat), so replaying a historical switch on remount
  // would overwrite the user's current picker. Seed handled ids from
  // `initialMessages`, and only queue a live switch (applied when the turn
  // ends — see `pendingModeSwitchRef`) so the picker stays aligned with
  // the backend's per-turn pin.
  const handledAccessGrantIdsRef = useRef<Set<string>>(new Set());
  const handledModeSwitchIdsRef = useRef<Set<string>>(collectSettledModeSwitchIds(initialMessages));
  const pendingModeSwitchRef = useRef<ConversationMode | null>(null);
  const handledFileWriteIdsRef = useRef<Set<string>>(new Set());
  const handledMoveIdsRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    const last = messages[messages.length - 1];
    if (!last || last.role !== "assistant") return;
    for (const block of last.blocks) {
      if (block.type !== "toolCall" || block.status !== "done") continue;
      if (block.name === "requestFullRepoAccess" && !handledAccessGrantIdsRef.current.has(block.id)) {
        handledAccessGrantIdsRef.current.add(block.id);
        void refreshAccessMode();
      }
      // Queue while the turn is in flight; apply immediately if somehow
      // already idle (keeps picker in sync if settle and `sending=false`
      // land in awkward orders). The flush effect below covers the normal
      // path: settle mid-turn → `sending` flips false → apply.
      if (block.name === "requestModeSwitch" && !handledModeSwitchIdsRef.current.has(block.id)) {
        handledModeSwitchIdsRef.current.add(block.id);
        if (block.result && block.result.tool === "modeSwitchRequested") {
          const nextMode = block.result.result.mode;
          if (sending) {
            pendingModeSwitchRef.current = nextMode;
          } else {
            pendingModeSwitchRef.current = null;
            onConversationModeChange(nextMode);
          }
        }
      }
      if (
        (block.name === "writeFile" ||
          block.name === "editFile" ||
          block.name === "deleteFile" ||
          block.name === "createDirectory" ||
          block.name === "deleteDirectory") &&
        !handledFileWriteIdsRef.current.has(block.id)
      ) {
        handledFileWriteIdsRef.current.add(block.id);
        // Every one of these `ToolResult` variants carries `{ path }` —
        // narrowed via the discriminant rather than a blind cast so a
        // future shape change here fails loudly instead of silently
        // passing `path: undefined` through.
        const path =
          block.result &&
          (block.result.tool === "fileWritten" ||
            block.result.tool === "fileEdited" ||
            block.result.tool === "fileDeleted" ||
            block.result.tool === "directoryCreated" ||
            block.result.tool === "directoryDeleted")
            ? block.result.result.path
            : null;
        if (path !== null) onFileWritten({ tool: block.name, path });
      }
      if (block.name === "move" && !handledMoveIdsRef.current.has(block.id)) {
        handledMoveIdsRef.current.add(block.id);
        if (block.result && block.result.tool === "moved") {
          const { from, to, updatedFiles } = block.result.result;
          onFileMoved({ from, to, updatedFiles });
        }
      }
    }
  }, [messages, sending, refreshAccessMode, onConversationModeChange, onFileWritten, onFileMoved]);

  // Flush a queued `requestModeSwitch` onto the session picker only after
  // the turn that approved it has fully finished — mirrors
  // `LoopCtx::conversation_mode` staying pinned for one `run_tool_loop`.
  useEffect(() => {
    if (sending) return;
    const pending = pendingModeSwitchRef.current;
    if (pending === null) return;
    pendingModeSwitchRef.current = null;
    onConversationModeChange(pending);
  }, [sending, onConversationModeChange]);

  const contextUsageRatio = contextLimit ? Math.min(1, contextTokens / contextLimit) : null;
  const [draft, setDraft] = useState("");
  // Фрагменты из «Добавить в чат», показанные как чипы над полем ввода —
  // не смешиваются с текстом черновика, но подставляются в отправляемое
  // сообщение целиком (см. `handleSend`).
  const [attachments, setAttachments] = useState<ChatAttachment[]>([]);
  // Свёрнут ли список вложений за кнопку «Все (N)» — сбрасывается при
  // отправке, чтобы следующая порция вложений снова начиналась свёрнутой.
  const [attachmentsExpanded, setAttachmentsExpanded] = useState(false);
  // Последний обработанный «Добавить в чат» — защита от повторного
  // срабатывания на том же объекте запроса при перерисовках (тот же приём,
  // что и `lastHandledInsertIdRef` в Editor.tsx).
  const lastHandledChatInsertIdRef = useRef(0);
  // The suggestion node the user most recently clicked (top-level or a
  // follow-up) — drives which `followUps` row (if any) shows above the
  // transcript. Tracking the node itself (not matching on message text)
  // survives the user editing the draft before sending, since some
  // suggestion `text` values are meant to be appended to rather than sent
  // verbatim.
  const [activeSuggestion, setActiveSuggestion] = useState<AssistantSuggestion | null>(null);
  // The suggestion whose input form is open, if any — a click on a chip that
  // declares `inputs` (or needs a repo-access upgrade) opens the form instead
  // of filling the composer straight away.
  const [formSuggestion, setFormSuggestion] = useState<AssistantSuggestion | null>(null);
  // Every value the user has typed into a suggestion form this chat, keyed by
  // `SuggestionInput.key` — lets a follow-up reuse e.g. the method name from
  // the first step instead of asking for it again.
  const [rememberedValues, setRememberedValues] = useState<Record<string, string>>({});
  // The chip the pointer/focus is on — its `hint` fills the description line
  // under the chip row, mirroring how the mode chips above describe the
  // hovered mode.
  const [hoveredSuggestion, setHoveredSuggestion] = useState<AssistantSuggestion | null>(null);
  const suggestionCtx = useMemo(
    () => buildSuggestionContext({ conversationMode, activeFilePath, hasUncommittedChanges }),
    [conversationMode, activeFilePath, hasUncommittedChanges],
  );
  // Each mode gets its own starting tasks — Plan mode offers plans, not
  // edits — so switching the mode chips above swaps this row.
  //
  // Every mode's row is computed (and rendered) rather than just the active
  // one: they are stacked in a single grid cell so the block keeps the
  // height of the tallest. The placeholder column is centred vertically, so
  // a row that grew by one chip would otherwise shove the mode chips —
  // the very thing the user just clicked — upward out from under the cursor.
  const suggestionsByMode = useMemo(
    () =>
      CHAT_MODE_OPTIONS.map((option) => ({
        mode: option.value,
        items: suggestionsForMode(ASSISTANT_SUGGESTIONS, {
          ...suggestionCtx,
          conversationMode: option.value,
        }),
      })),
    [suggestionCtx],
  );
  const hasAnySuggestion = suggestionsByMode.some((group) => group.items.length > 0);
  // The hint line describes a chip in the row above it; when the row is
  // swapped out (mode change) the old hint would otherwise linger under
  // chips it has nothing to do with.
  useEffect(() => {
    setHoveredSuggestion(null);
  }, [conversationMode]);
  const followUpSuggestions = useMemo(
    () => visibleSuggestions(activeSuggestion?.followUps ?? [], suggestionCtx),
    [activeSuggestion, suggestionCtx],
  );
  const showFollowUpBar =
    followUpSuggestionsEnabled && messages.length > 0 && followUpSuggestions.length > 0;

  // Shown while the last search in this chat ran without its semantic tier
  // — the assistant keeps working, but on a narrower search than usual, and
  // that is worth knowing *before* trusting (or continuing) the answer.
  // Cheap enough to recompute per render: it stops at the newest search.
  const embeddingsUnavailable = searchIsDegraded(messages);

  const messagesRef = useRef<HTMLDivElement>(null);
  const chatInputRef = useRef<HTMLTextAreaElement>(null);
  // Auto-follow state for the transcript scroll — separate from React state
  // where possible (`pinnedToBottomRef`) since it's read inside the
  // high-frequency `messages` effect below and mustn't itself trigger a
  // re-render; `showJumpToBottom` is the one bit that does need to be state,
  // since it drives the floating button's visibility.
  const pinnedToBottomRef = useRef(true);
  const didMountScrollRef = useRef(false);
  const followFrameRef = useRef<number | null>(null);
  const [showJumpToBottom, setShowJumpToBottom] = useState(false);

  // Model picker — reads the catalog saved in Settings (`knownModels`).
  // Choosing a model persists `LlmProviderConfig.model` (the provider-wide
  // default used by chat, selection AI, compaction, and memory extraction —
  // see `llm_session::resolve` / `effective_model` on the Rust side).
  const [modelSelectOpen, setModelSelectOpen] = useState(false);
  const modelSelectRef = useRef<HTMLDivElement>(null);
  const catalogModels = activeProvider?.knownModels ?? [];

  // Sticks the transcript to its bottom edge as new messages/deltas arrive,
  // the way Cursor/ChatGPT do — but only while the user hasn't scrolled up
  // to read something earlier (`pinnedToBottomRef`, kept current by
  // `handleMessagesScroll` below). Follow-up work is coalesced into one
  // animation frame: repeatedly starting independent smooth scrolls for
  // every streamed token makes the browser fight the user's wheel gesture.
  const scheduleFollowToBottom = useCallback(() => {
    if (!pinnedToBottomRef.current || followFrameRef.current !== null) return;
    followFrameRef.current = requestAnimationFrame(() => {
      followFrameRef.current = null;
      const el = messagesRef.current;
      if (!el || !pinnedToBottomRef.current) return;
      el.scrollTop = el.scrollHeight;
    });
  }, []);

  useEffect(() => {
    const el = messagesRef.current;
    if (!el) return;
    if (!didMountScrollRef.current) {
      el.scrollTop = el.scrollHeight;
      didMountScrollRef.current = true;
      return;
    }
    scheduleFollowToBottom();
  }, [messages, scheduleFollowToBottom]);

  // A tool card can grow after `messages` has already been updated (for
  // example when a tool result or a streamed block gets rendered). Observe
  // message bubbles so those layout changes keep the transcript pinned too.
  useEffect(() => {
    const el = messagesRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;

    const observer = new ResizeObserver(scheduleFollowToBottom);
    observer.observe(el);
    for (const child of Array.from(el.children)) observer.observe(child);
    return () => observer.disconnect();
  }, [messages.length, scheduleFollowToBottom]);

  useEffect(() => {
    return () => {
      if (followFrameRef.current !== null) {
        cancelAnimationFrame(followFrameRef.current);
      }
    };
  }, []);

  // Keep the follow mode detached until the viewport is genuinely at the
  // bottom. A large threshold makes a downward wheel gesture re-attach early,
  // so the next streamed delta pulls the message out from under the cursor.
  const SCROLL_BOTTOM_THRESHOLD_PX = 4;

  const handleMessagesScroll = () => {
    const el = messagesRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight <= SCROLL_BOTTOM_THRESHOLD_PX;
    if (atBottom) {
      // Re-attach when the user or the follow loop reaches the bottom. A
      // content resize can make `atBottom` false without moving scrollTop;
      // that must not detach an already pinned transcript.
      pinnedToBottomRef.current = true;
      setShowJumpToBottom(false);
    }
  };

  const handleMessagesWheel = (event: WheelEvent<HTMLDivElement>) => {
    if (event.deltaY > 0) {
      setShowJumpToBottom(false);
      return;
    }
    if (event.deltaY === 0) return;
    pinnedToBottomRef.current = false;
    setShowJumpToBottom(true);
    if (followFrameRef.current !== null) {
      cancelAnimationFrame(followFrameRef.current);
      followFrameRef.current = null;
    }
  };

  const handleJumpToBottom = () => {
    const el = messagesRef.current;
    if (!el) return;
    pinnedToBottomRef.current = true;
    setShowJumpToBottom(false);
    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  };

  // «Добавить в чат» из редактора — добавляем чип вложения (не трогаем
  // текст черновика) и фокусим ввод. Пустые выделения (по факту не доходят
  // сюда) игнорируем. `onChatInsertHandled` clears the request in App right
  // after — this component remounts on chat switch (`key={currentChatId}`
  // in AssistantPanel), so leaving the request live would replay it as a
  // duplicate attachment on the next chat; `lastHandledChatInsertIdRef` only
  // guards against a double-fire within this one mount.
  useEffect(() => {
    if (!chatInsertRequest || chatInsertRequest.id === lastHandledChatInsertIdRef.current) return;
    lastHandledChatInsertIdRef.current = chatInsertRequest.id;
    if (chatInsertRequest.text.trim()) {
      setAttachments((prev) => [
        ...prev,
        { id: chatInsertRequest.id, text: chatInsertRequest.text, filePath: chatInsertRequest.filePath },
      ]);
      chatInputRef.current?.focus();
    }
    onChatInsertHandled?.();
  }, [chatInsertRequest, onChatInsertHandled]);

  const lastHandledAssistantSendIdRef = useRef<number | null>(null);
  useEffect(() => {
    if (!assistantSendRequest || assistantSendRequest.id === lastHandledAssistantSendIdRef.current) {
      return;
    }
    lastHandledAssistantSendIdRef.current = assistantSendRequest.id;
    runAssistantSend(
      assistantSendRequest.text,
      assistantSendRequest.conversationMode ?? "agent",
    );
    onAssistantSendHandled?.();
  }, [assistantSendRequest, runAssistantSend, onAssistantSendHandled]);

  const lastHandledAssistantDraftIdRef = useRef<number | null>(null);
  useEffect(() => {
    if (!assistantDraftRequest || assistantDraftRequest.id === lastHandledAssistantDraftIdRef.current) {
      return;
    }
    lastHandledAssistantDraftIdRef.current = assistantDraftRequest.id;
    const targetMode = assistantDraftRequest.conversationMode;
    if (targetMode && targetMode !== conversationMode) {
      onConversationModeChange(targetMode);
    }
    setDraft(assistantDraftRequest.text);
    requestAnimationFrame(() => {
      const el = chatInputRef.current;
      if (!el) return;
      el.focus();
      el.setSelectionRange(el.value.length, el.value.length);
    });
    onAssistantDraftHandled?.();
  }, [
    assistantDraftRequest,
    conversationMode,
    onConversationModeChange,
    onAssistantDraftHandled,
  ]);

  // Reset the fetched model list (and close the menu) whenever the active
  // provider itself changes, so a stale list from a different provider
  // never briefly shows.
  useEffect(() => {
    setModelSelectOpen(false);
  }, [providerId]);

  useEffect(() => {
    if (!modelSelectOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!modelSelectRef.current?.contains(event.target as Node)) setModelSelectOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setModelSelectOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [modelSelectOpen]);

  const handleToggleModelSelect = () => {
    setModelSelectOpen((open) => {
      const next = !open;
      if (next) {
        void refreshLlmSetup();
      }
      return next;
    });
  };

  const handleSelectModel = (value: string) => {
    if (!providerId) return;
    setModelSelectOpen(false);
    // Provider-wide pin — not per-chat — so every LLM caller that resolves
    // this provider id picks up the same model on its next request.
    void updateProviderConfig(providerId, { model: value === AUTO_MODEL_VALUE ? null : value });
  };

  // Fills the composer from a suggestion, applying whatever the suggestion
  // declares it needs. Never sends: the user reads the finished prompt and
  // presses Enter themselves, which also means the access-mode round trip
  // below has settled long before the turn actually leaves.
  const applySuggestion = (suggestion: AssistantSuggestion, values: Record<string, string>) => {
    if (suggestion.mode && suggestion.mode !== conversationMode) {
      // Silent: the mode chips right above the suggestions show the result.
      onConversationModeChange(suggestion.mode);
    }
    if (needsAccessUpgrade(suggestion, accessMode)) {
      // Only ever reached through the form's explicit «Включить доступ и
      // вставить» button — see `handleSuggestionClick`.
      onAccessModeChange("fullRepo");
    }
    setRememberedValues((prev) => ({ ...prev, ...values }));
    setDraft(renderSuggestionText(suggestion, values));
    setActiveSuggestion(suggestion);
    requestAnimationFrame(() => {
      const el = chatInputRef.current;
      if (!el) return;
      el.focus();
      el.setSelectionRange(el.value.length, el.value.length);
    });
  };

  const handleSuggestionClick = (suggestion: AssistantSuggestion) => {
    if (needsSuggestionForm(suggestion, accessMode)) {
      setFormSuggestion(suggestion);
      return;
    }
    applySuggestion(suggestion, {});
  };

  const handleSuggestionFormSubmit = (values: Record<string, string>) => {
    if (!formSuggestion) return;
    applySuggestion(formSuggestion, values);
    setFormSuggestion(null);
  };

  const handleSend = () => {
    const text = draft.trim();
    if (sending) {
      if (!text) return;
      setDraft("");
      void steerChat(text).catch((error) => {
        console.error("Не удалось добавить уточнение", error);
        setDraft((current) => current || text);
      });
      return;
    }
    if (!text && attachments.length === 0) return;
    // Attachment chips carry no text of their own in the draft — the model
    // still needs their content, so it's woven in here, at the point the
    // message actually leaves, ahead of whatever the user typed.
    const quotes = attachments.map((a) => formatSelectionForChat(a.text, a.filePath));
    const combined = [...quotes, text].filter(Boolean).join("\n\n");
    setDraft("");
    setAttachments([]);
    setAttachmentsExpanded(false);
    // Sending a message always means "follow the reply", even if the user
    // had scrolled up to reread something earlier in the transcript.
    pinnedToBottomRef.current = true;
    setShowJumpToBottom(false);
    void sendMessage(combined);
  };

  return (
    <>
      <TodoProgressWidget tasks={todos} onClearAll={sending ? undefined : clearTodos} />
      {activePlanId ? <PlanProgressWidget planId={activePlanId} refreshKey={messages.length} /> : null}
      <div className="assistant-chat-scroll-shell">
        <div
          className="assistant-chat-messages"
          ref={messagesRef}
          onScroll={handleMessagesScroll}
          onWheel={handleMessagesWheel}
        >
        {messages.length === 0 ? (
          <div className="assistant-chat-placeholder">
            <Sparkles className="assistant-chat-placeholder-icon" size={20} strokeWidth={1.5} aria-hidden />
            <p className="assistant-chat-placeholder-title">Привет! Я Атлас</p>
            <div className="assistant-chat-stack is-inline">
              {CHAT_MODE_OPTIONS.map((option) => (
                <p
                  key={option.value}
                  className="assistant-chat-placeholder-desc"
                  data-inactive={option.value === conversationMode ? undefined : "true"}
                  aria-hidden={option.value === conversationMode ? undefined : true}
                >
                  {option.greeting}
                </p>
              ))}
            </div>

            <div className="assistant-chat-mode-chips" role="group" aria-label="Режим ассистента">
              {CHAT_MODE_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  className={`assistant-chat-mode-chip${conversationMode === option.value ? " is-active" : ""}`}
                  title={option.title}
                  aria-pressed={conversationMode === option.value}
                  disabled={sending}
                  onClick={() => onConversationModeChange(option.value)}
                >
                  {option.label}
                </button>
              ))}
            </div>

            {hasAnySuggestion ? (
              <div className="assistant-chat-placeholder-suggestions">
                <p className="assistant-chat-placeholder-suggestions-label">Шаблоны задач</p>
                <div className="assistant-chat-stack">
                  {suggestionsByMode.map(({ mode, items }) => (
                    <div
                      key={mode}
                      className="assistant-chat-suggestions"
                      data-inactive={mode === conversationMode ? undefined : "true"}
                      aria-hidden={mode === conversationMode ? undefined : true}
                    >
                      {items.map((s) => (
                        <AssistantSuggestionChip
                          key={s.id}
                          suggestion={s}
                          className="assistant-suggestion-chip"
                          onClick={() => handleSuggestionClick(s)}
                          onHoverChange={setHoveredSuggestion}
                        />
                      ))}
                    </div>
                  ))}
                </div>
                {/* Reserved height, so pointing at a chip doesn't shift the
                    row it is in — the placeholder column is centred. */}
                <p className="assistant-chat-suggestion-desc" aria-live="polite">
                  {hoveredSuggestion?.hint ?? "\u00a0"}
                </p>
              </div>
            ) : null}
          </div>
        ) : (
          messages.map((m) => {
            if (m.role === "assistant" && m.isCompactionNotice) {
              return <AssistantCompactionNotice key={m.id} message={m} />;
            }
            const failed = m.role === "assistant" && Boolean(m.failed);
            const stopped = m.role === "assistant" && Boolean(m.cancelled);
            // Which blocks the model may still be writing into — not simply
            // "the last one": a provider that interleaves reasoning with its
            // answer leaves both blocks open at once, the reasoning one above.
            const liveBlockIds = m.role === "assistant" && m.streaming
              ? openStreamingBlockIds(m.blocks)
              : EMPTY_LIVE_BLOCK_IDS;
            // "Open for more deltas" and "being written right now" are not
            // the same thing: a reasoning block stays open for the whole
            // round (providers may interleave), so without `liveKind` the
            // thinking card shimmered — and its timer ran — for as long as
            // the answer below it streamed.
            const liveKind = m.role === "assistant" ? m.liveKind : undefined;
            return (
              <div
                key={m.id}
                className={`assistant-chat-message ${m.role}${failed ? " failed" : ""}${stopped ? " cancelled" : ""}`}
              >
                {m.role === "assistant" ? (
                  m.blocks.length === 0 && m.streaming ? (
                    <AssistantThinkingIndicator />
                  ) : (
                    <div className="assistant-chat-blocks">
                      {groupBlocksForRender(m.blocks).map((item) =>
                        item.kind === "artifactGroup" ? (
                          <AssistantArtifactCard
                            key={item.blocks[0]!.id}
                            blocks={item.blocks}
                            chatId={chatId}
                            onAnswer={answerArtifact}
                            onDefer={(id) => decideToolCall(id, false, false)}
                          />
                        ) : item.kind === "askGroup" ? (
                          <AssistantAskUserCard
                            key={item.blocks[0]!.id}
                            blocks={item.blocks}
                            onAnswer={answerAskUser}
                            onSkip={(id) => decideToolCall(id, false, false)}
                          />
                        ) : item.kind === "approvalGroup" ? (
                          <AssistantToolApprovalGroup
                            key={item.blocks[0]!.id}
                            blocks={item.blocks}
                            docsRoot={docsRoot}
                            repoRoot={repoRoot}
                            onDecide={decideToolCall}
                          />
                        ) : item.block.type === "reasoning" ? (
                          <AssistantReasoningBlock
                            key={item.block.id}
                            block={item.block}
                            thinking={liveBlockIds.has(item.block.id) && liveKind === "reasoning"}
                          />
                        ) : item.block.type === "text" ? (
                          <AssistantMarkdown
                            key={item.block.id}
                            content={item.block.content}
                            streaming={liveBlockIds.has(item.block.id) && liveKind !== "reasoning"}
                          />
                        ) : item.block.type === "steer" ? (
                          <AssistantSteerBlock key={item.block.id} block={item.block} />
                        ) : isPlanToolBlock(item.block) ? (
                          <AssistantPlanCard
                            key={item.block.id}
                            block={item.block}
                            startDisabled={sending}
                            onOpenPlan={(planId) => {
                              window.dispatchEvent(
                                new CustomEvent("atlas-open-plan", { detail: { planId } }),
                              );
                            }}
                            onStartPlan={startPlan}
                          />
                        ) : isTicketToolBlock(item.block) ? (
                          <AssistantTicketCard
                            key={item.block.id}
                            block={item.block}
                            onOpenArtifact={openArtifactTab}
                          />
                        ) : isVisualToolBlock(item.block) ? (
                          <AssistantVisualCard
                            key={item.block.id}
                            block={item.block}
                            turnActive={m.streaming === true}
                            onOpenVisual={openVisualTab}
                            onRenderError={(note) => void noteLlmChat(note)}
                            onRedraw={(request) => {
                              // While the turn runs this is a steer (it
                              // lands in the round already in flight);
                              // afterwards it has to be a real message,
                              // since the steering queue is cleared when a
                              // fresh turn starts.
                              if (sending) void steerChat(request);
                              else void sendMessage(request);
                            }}
                          />
                        ) : (
                          <AssistantToolCallBlock key={item.block.id} block={item.block} />
                        ),
                      )}
                      {m.streaming && !lastBlockShowsLiveProgress(m.blocks) ? (
                        <AssistantThinkingIndicator />
                      ) : null}
                      {stopped ? <div className="assistant-chat-cancelled-note">Остановлено пользователем</div> : null}
                      {failed ? (
                        <div className="assistant-chat-error-card">
                          <AlertCircle size={13} aria-hidden />
                          <span>{m.errorMessage ?? "Не удалось получить ответ"}</span>
                          {m.contextLengthExceeded ? (
                            <button
                              type="button"
                              className="assistant-chat-error-retry"
                              onClick={() => retryWithCompaction(m.id)}
                            >
                              Сжать историю и повторить
                            </button>
                          ) : null}
                        </div>
                      ) : null}
                      {typeof m.durationMs === "number" ? (
                        <div className="assistant-chat-duration">
                          Готово за {formatElapsedDuration(Math.round(m.durationMs / 1000))}
                        </div>
                      ) : null}
                    </div>
                  )
                ) : (
                  <AssistantUserMessage content={m.content} />
                )}
              </div>
            );
          })
        )}
        </div>
        {showJumpToBottom ? (
          <button
            type="button"
            className="assistant-scroll-to-bottom"
            onClick={handleJumpToBottom}
          >
            <ArrowDown size={12} strokeWidth={2} aria-hidden />
            <span>Вниз</span>
          </button>
        ) : null}
      </div>
      <div className="assistant-model-bar">
        <ChatModeSelect
          mode={conversationMode}
          onChange={(next) => {
            // Reported here, at the control itself, rather than inside
            // `onConversationModeChange` — that callback is also how an
            // assistant-requested switch gets applied, and the two must
            // not be conflated. The assistant's side is reported from
            // `useLlmChat`, where the user's answer is known.
            void trackMetric(
              METRICS.ASSISTANT.SWITCH_CONVERSATION_MODE,
              undefined,
              { label: "user", property: next },
            );
            onConversationModeChange(next);
          }}
          disabled={sending}
        />
        <AccessModeToggle
          mode={accessMode}
          onChange={(next) => {
            void trackMetric(METRICS.ASSISTANT.SWITCH_ACCESS_MODE, undefined, {
              label: "user",
              property: next,
            });
            onAccessModeChange(next);
          }}
          disabled={sending || accessModeBusy}
        />
        <div className="clone-select assistant-model-select" ref={modelSelectRef}>
          <button
            type="button"
            className={`clone-select-trigger${modelSelectOpen ? " is-open" : ""}`}
            aria-haspopup="listbox"
            aria-expanded={modelSelectOpen}
            disabled={sending}
            onClick={handleToggleModelSelect}
          >
            <span className="clone-select-value">
              <span className="clone-select-path">{activeProvider?.model ?? AUTO_MODEL_LABEL}</span>
            </span>
            <span className="clone-select-chevron" aria-hidden>
              ▾
            </span>
          </button>
          {modelSelectOpen ? (
            <div className="clone-select-menu" role="listbox">
              <button
                type="button"
                role="option"
                aria-selected={!activeProvider?.model}
                className={`clone-select-option${!activeProvider?.model ? " is-active" : ""}`}
                onClick={() => handleSelectModel(AUTO_MODEL_VALUE)}
              >
                <span className="clone-select-path">{AUTO_MODEL_LABEL}</span>
              </button>
              {catalogModels.length === 0 && !activeProvider?.model ? (
                <div className="clone-select-option is-disabled" aria-disabled>
                  <span className="clone-select-path">{CHAT_MODEL_CATALOG_EMPTY_HINT}</span>
                </div>
              ) : null}
              {activeProvider?.model && !catalogModels.includes(activeProvider.model) ? (
                <button
                  type="button"
                  role="option"
                  aria-selected
                  className="clone-select-option is-active"
                  onClick={() => handleSelectModel(activeProvider.model as string)}
                >
                  <span className="clone-select-path">{activeProvider.model}</span>
                </button>
              ) : null}
              {catalogModels.map((id) => (
                <button
                  key={id}
                  type="button"
                  role="option"
                  aria-selected={id === activeProvider?.model}
                  className={`clone-select-option${id === activeProvider?.model ? " is-active" : ""}`}
                  onClick={() => handleSelectModel(id)}
                >
                  <span className="clone-select-path">{id}</span>
                </button>
              ))}
            </div>
          ) : null}
        </div>
        {contextLimit !== null ? (
          <div
            className={`assistant-context-bar${contextUsageRatio !== null && contextUsageRatio >= CONTEXT_NEAR_LIMIT_RATIO ? " near-limit" : ""}`}
            title={contextUsageTitle(contextTokens, contextLimit, lastRequestTokens, sending)}
          >
            <svg className="assistant-context-ring" width="20" height="20" viewBox="0 0 20 20" aria-hidden>
              <circle className="assistant-context-ring-track" cx="10" cy="10" r={CONTEXT_RING_RADIUS} />
              <circle
                className="assistant-context-ring-fill"
                cx="10"
                cy="10"
                r={CONTEXT_RING_RADIUS}
                strokeDasharray={CONTEXT_RING_CIRCUMFERENCE}
                strokeDashoffset={CONTEXT_RING_CIRCUMFERENCE * (1 - (contextUsageRatio ?? 0))}
              />
            </svg>
            <span className="assistant-context-bar-label">
              {formatTokenCount(contextTokens)} / {formatTokenCount(contextLimit)}
            </span>
          </div>
        ) : null}
      </div>
      {embeddingsUnavailable ? (
        <div className="assistant-degraded-bar" role="status">
          <AlertCircle size={13} strokeWidth={1.75} aria-hidden />
          <span>
            API эмбеддингов недоступен — поиск идёт только по именам и тексту, результаты могут быть хуже.
          </span>
        </div>
      ) : null}
      {showFollowUpBar ? (
        <div className="assistant-followup-bar" role="group" aria-label="Похожие предложения">
          <Sparkles className="assistant-followup-bar-icon" size={12} strokeWidth={1.75} aria-hidden />
          <div className="assistant-followup-bar-chips">
            {followUpSuggestions.map((s) => (
              <AssistantSuggestionChip
                key={s.id}
                suggestion={s}
                className="assistant-followup-chip"
                disabled={sending}
                onClick={() => handleSuggestionClick(s)}
              />
            ))}
          </div>
          <button
            type="button"
            className="assistant-followup-bar-dismiss"
            aria-label="Скрыть предложения"
            onClick={() => setActiveSuggestion(null)}
          >
            <X size={12} aria-hidden />
          </button>
        </div>
      ) : null}
      <div className={`assistant-chat-input-row${showFollowUpBar ? " has-followups" : ""}`}>
        <div className="assistant-chat-input-wrap">
          {pendingSteers.length > 0 ? (
            <div className="assistant-steer-pending-list" role="status" aria-label="Уточнения в очереди">
              {pendingSteers.map((text, index) => (
                <div className="assistant-steer-pending" key={`${index}-${text}`}>
                  <Clock3 size={12} strokeWidth={1.75} aria-hidden />
                  <span className="assistant-steer-pending-text">{text}</span>
                  <span className="assistant-steer-pending-label">В очереди</span>
                </div>
              ))}
            </div>
          ) : null}
          {attachments.length === 0 ? null : attachments.length <= ATTACHMENTS_INLINE_LIMIT ? (
            <div className="assistant-chat-attachments" role="list">
              {attachments.map((attachment) => (
                <AttachmentChip
                  key={attachment.id}
                  attachment={attachment}
                  variant="chip"
                  onRemove={() => setAttachments((prev) => prev.filter((a) => a.id !== attachment.id))}
                />
              ))}
            </div>
          ) : !attachmentsExpanded ? (
            <div className="assistant-chat-attachments" role="list">
              {attachments.slice(0, ATTACHMENTS_INLINE_LIMIT).map((attachment) => (
                <AttachmentChip
                  key={attachment.id}
                  attachment={attachment}
                  variant="chip"
                  onRemove={() => setAttachments((prev) => prev.filter((a) => a.id !== attachment.id))}
                />
              ))}
              <button
                type="button"
                className="assistant-chat-attachments-toggle"
                onClick={() => setAttachmentsExpanded(true)}
              >
                Все ({attachments.length})
              </button>
            </div>
          ) : (
            <div className="assistant-chat-attachments-list">
              <button
                type="button"
                className="assistant-chat-attachments-toggle"
                onClick={() => setAttachmentsExpanded(false)}
              >
                <ChevronUp size={12} aria-hidden />
                Свернуть
              </button>
              <div className="assistant-chat-attachments-list-items" role="list">
                {attachments.map((attachment) => (
                  <AttachmentChip
                    key={attachment.id}
                    attachment={attachment}
                    variant="row"
                    onRemove={() => setAttachments((prev) => prev.filter((a) => a.id !== attachment.id))}
                  />
                ))}
              </div>
            </div>
          )}
          <textarea
            ref={chatInputRef}
            className="assistant-chat-input"
            rows={CHAT_INPUT_ROWS}
            value={draft}
            placeholder={
              sending
                ? "Уточнение…\n(Enter — добавить в работу, Shift+Enter — новая строка)"
                : "Спросите что-нибудь…\n(Enter — отправить, Shift+Enter — новая строка)"
            }
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                handleSend();
              }
            }}
          />
          <div className="assistant-chat-input-tools">
            {sending ? (
              <button
                type="button"
                className="assistant-chat-stop"
                aria-label="Остановить"
                title="Остановить ответ ассистента"
                onClick={stopChat}
              >
                <Square size={13} strokeWidth={1.75} fill="currentColor" aria-hidden />
              </button>
            ) : (
              <button
                type="button"
                className="assistant-chat-send"
                disabled={!draft.trim() && attachments.length === 0}
                aria-label="Отправить"
                onClick={handleSend}
              >
                <Send size={15} strokeWidth={1.75} aria-hidden />
              </button>
            )}
          </div>
        </div>
      </div>
      {formSuggestion ? (
        <AssistantSuggestionModal
          suggestion={formSuggestion}
          initialValues={prefillValues(formSuggestion, rememberedValues, activeFilePath)}
          accessMode={accessMode}
          onCancel={() => setFormSuggestion(null)}
          onSubmit={handleSuggestionFormSubmit}
        />
      ) : null}
    </>
  );
}
