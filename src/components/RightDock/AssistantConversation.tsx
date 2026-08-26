import { useEffect, useRef, useState } from "react";
import { AlertCircle, ArrowDown, ChevronUp, FileText, Send, Sparkles, Square, X } from "lucide-react";
import { useLlmChat } from "../../hooks/useLlmChat";
import {
  ASSISTANT_SUGGESTIONS,
  AUTO_MODEL_LABEL,
  AUTO_MODEL_VALUE,
  CHAT_INPUT_ROWS,
  CONTEXT_NEAR_LIMIT_RATIO,
  PLAN_EXECUTION_START_TEXT,
} from "../../lib/assistantConfig";
import type { AssistantSuggestion } from "../../lib/assistantConfig";
import type { AiAccessMode, ConversationMode, LlmToolDefinition, Task } from "../../lib/aiTools";
import { groupBlocksForRender, type ChatMessage } from "../../lib/chatBlocks";
import type { LlmModelInfo, LlmProviderConfig, PendingApproval, ResolvedLlmProvider } from "../../lib/llm";
import type { SpecsRepoInfo } from "../../lib/openapi";
import type { UpdatedReference } from "../../lib/project";
import { AssistantMarkdown } from "./AssistantMarkdown";
import { AssistantAskUserCard } from "./AssistantAskUserCard";
import { AssistantReasoningBlock } from "./AssistantReasoningBlock";
import { AssistantToolApprovalGroup } from "./AssistantToolApprovalGroup";
import { AssistantToolCallBlock } from "./AssistantToolCallBlock";
import { AssistantPlanCard, isPlanToolBlock } from "./AssistantPlanCard";
import { TodoProgressWidget } from "./TodoProgressWidget";
import { PlanProgressWidget } from "./PlanProgressWidget";

const CHAT_MODE_OPTIONS: { value: ConversationMode; label: string; title: string }[] = [
  { value: "agent", label: "Агент", title: "Полный набор инструментов — исследует и вносит изменения." },
  { value: "plan", label: "План", title: "Только чтение — исследует и предлагает план, без изменений." },
  { value: "question", label: "Вопрос", title: "Лёгкий режим для точечных вопросов, без изменений." },
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

// Geometry for the context-usage ring (see `.assistant-context-ring` in
// AssistantPanel.css) — an SVG circle's stroke-dasharray/-dashoffset trick,
// not a library: the fill circle's dash length equals the full
// circumference and its offset shrinks as usage grows, so the visible arc
// sweeps clockwise from 12 o'clock (the ring itself is rotated -90deg in
// CSS to make that the start point).
const CONTEXT_RING_RADIUS = 8;
const CONTEXT_RING_CIRCUMFERENCE = 2 * Math.PI * CONTEXT_RING_RADIUS;

type AssistantConversationProps = {
  /** The chat this instance owns — the parent (`AssistantPanel`) is
   * expected to remount this component (via `key={chatId}`) whenever the
   * active chat changes, which is what resets every bit of per-conversation
   * state below (including `useLlmChat`'s own internal refs — trust set,
   * in-flight approval timers) cleanly, without a manual reset effect that
   * could race an in-flight turn. */
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
  loadModels: (providerId: string) => Promise<LlmModelInfo[]>;
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
};

/** The actual per-conversation surface: message transcript, model picker,
 * context-usage ring, and the input row — everything `AssistantPanel.tsx`
 * used to render below its access-mode toggle before chat history split it
 * into an outer shell (owns which chat is active) plus this remountable
 * body (owns one conversation). Always rendered with an LLM provider ready
 * — the parent gates that, so unlike the old single-file component this
 * one doesn't need its own `llmReady` checks. */
export function AssistantConversation({
  initialMessages,
  initialTodos,
  initialActivePlanId,
  initialPendingResume,
  onTurnSettled,
  onTurnPaused,
  onSendingChange,
  providerId,
  accessMode,
  conversationMode,
  onConversationModeChange,
  specsRepoInfo,
  toolDefinitions,
  docsRootRelativeToRepo,
  docsRoot,
  repoRoot,
  activeFilePath,
  onFileWritten,
  onFileMoved,
  refreshAccessMode,
  activeProvider,
  updateProviderConfig,
  loadModels,
  followUpSuggestionsEnabled,
  taskDoneSoundEnabled,
  needAnswerSoundEnabled,
  chatInsertRequest,
  onChatInsertHandled,
}: AssistantConversationProps) {
  const contextLimit = activeProvider?.limit?.context ?? null;

  const {
    messages,
    sending,
    sendMessage,
    retryWithCompaction,
    stopChat,
    contextTokens,
    decideToolCall,
    answerAskUser,
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
  const followUpSuggestions = activeSuggestion?.followUps;
  const showFollowUpBar =
    followUpSuggestionsEnabled && messages.length > 0 && Boolean(followUpSuggestions?.length);

  const messagesRef = useRef<HTMLDivElement>(null);
  const chatInputRef = useRef<HTMLTextAreaElement>(null);
  // Auto-follow state for the transcript scroll — separate from React state
  // where possible (`pinnedToBottomRef`) since it's read inside the
  // high-frequency `messages` effect below and mustn't itself trigger a
  // re-render; `showJumpToBottom` is the one bit that does need to be state,
  // since it drives the floating button's visibility.
  const pinnedToBottomRef = useRef(true);
  const didMountScrollRef = useRef(false);
  const [showJumpToBottom, setShowJumpToBottom] = useState(false);

  // Model picker — reuses the same `.clone-select*` trigger/menu pattern as
  // `LlmTab.tsx`'s own model dropdown (and writes to the same underlying
  // setting via `updateProviderConfig`), so picking a model here or in
  // Settings stays a single source of truth.
  const [models, setModels] = useState<LlmModelInfo[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelSelectOpen, setModelSelectOpen] = useState(false);
  const modelSelectRef = useRef<HTMLDivElement>(null);

  // Sticks the transcript to its bottom edge as new messages/deltas arrive,
  // the way Cursor/ChatGPT do — but only while the user hasn't scrolled up
  // to read something earlier (`pinnedToBottomRef`, kept current by
  // `handleMessagesScroll` below). The very first paint for this
  // conversation (mount, or a chat switch — this component remounts via
  // `key={chatId}`) jumps instantly instead of animating from the top of a
  // potentially long restored history; every following-along update after
  // that animates, since deltas arrive many times a second and repeatedly
  // retargeting a `behavior: "smooth"` scroll is what gives the streaming
  // text its continuous "catching up" motion instead of a jittery snap per
  // token.
  useEffect(() => {
    const el = messagesRef.current;
    if (!el) return;
    if (!didMountScrollRef.current) {
      el.scrollTop = el.scrollHeight;
      didMountScrollRef.current = true;
      return;
    }
    if (pinnedToBottomRef.current) {
      el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
    }
  }, [messages]);

  const SCROLL_BOTTOM_THRESHOLD_PX = 48;

  const handleMessagesScroll = () => {
    const el = messagesRef.current;
    if (!el) return;
    const pinned = el.scrollHeight - el.scrollTop - el.clientHeight <= SCROLL_BOTTOM_THRESHOLD_PX;
    pinnedToBottomRef.current = pinned;
    setShowJumpToBottom(!pinned);
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

  // Reset the fetched model list (and close the menu) whenever the active
  // provider itself changes, so a stale list from a different provider
  // never briefly shows.
  useEffect(() => {
    setModels([]);
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
      if (next && models.length === 0 && !modelsLoading && providerId) {
        setModelsLoading(true);
        loadModels(providerId)
          .then(setModels)
          .catch(() => {})
          .finally(() => setModelsLoading(false));
      }
      return next;
    });
  };

  const handleSelectModel = (value: string) => {
    if (!providerId) return;
    setModelSelectOpen(false);
    void updateProviderConfig(providerId, { model: value === AUTO_MODEL_VALUE ? null : value });
  };

  const handleSuggestionClick = (suggestion: AssistantSuggestion) => {
    setDraft(suggestion.text);
    setActiveSuggestion(suggestion);
    requestAnimationFrame(() => {
      const el = chatInputRef.current;
      if (!el) return;
      el.focus();
      el.setSelectionRange(el.value.length, el.value.length);
    });
  };

  const handleSend = () => {
    const text = draft.trim();
    if ((!text && attachments.length === 0) || sending) return;
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
      <div className="assistant-chat-messages" ref={messagesRef} onScroll={handleMessagesScroll}>
        {messages.length === 0 ? (
          <div className="assistant-chat-placeholder">
            <Sparkles size={22} strokeWidth={1.5} aria-hidden />
            <p className="assistant-chat-placeholder-title">Ассистент готов</p>
            <p className="assistant-chat-placeholder-desc">Задайте вопрос о документации проекта.</p>
            <div className="assistant-chat-suggestions">
              {ASSISTANT_SUGGESTIONS.map((s) => (
                <button
                  key={s.id}
                  type="button"
                  className="assistant-suggestion-chip"
                  onClick={() => handleSuggestionClick(s)}
                >
                  {s.label}
                </button>
              ))}
            </div>
          </div>
        ) : (
          messages.map((m) => {
            if (m.role === "assistant" && m.isCompactionNotice) {
              return (
                <div key={m.id} className="assistant-chat-compaction-notice">
                  <span>{m.blocks[0]?.type === "text" ? m.blocks[0].content : ""}</span>
                </div>
              );
            }
            const failed = m.role === "assistant" && Boolean(m.failed);
            const stopped = m.role === "assistant" && Boolean(m.cancelled);
            return (
              <div
                key={m.id}
                className={`assistant-chat-message ${m.role}${failed ? " failed" : ""}${stopped ? " cancelled" : ""}`}
              >
                {m.role === "assistant" ? (
                  m.blocks.length === 0 && m.streaming ? (
                    <span className="assistant-chat-typing" aria-label="Ассистент печатает…">
                      <span />
                      <span />
                      <span />
                    </span>
                  ) : (
                    <div className="assistant-chat-blocks">
                      {groupBlocksForRender(m.blocks).map((item, i, arr) =>
                        item.kind === "askGroup" ? (
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
                            thinking={Boolean(m.streaming) && i === arr.length - 1}
                          />
                        ) : item.block.type === "text" ? (
                          <AssistantMarkdown
                            key={item.block.id}
                            content={item.block.content}
                            streaming={Boolean(m.streaming) && i === arr.length - 1}
                          />
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
                        ) : (
                          <AssistantToolCallBlock key={item.block.id} block={item.block} />
                        ),
                      )}
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
                    </div>
                  )
                ) : (
                  m.content
                )}
              </div>
            );
          })
        )}
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
        <ChatModeSelect mode={conversationMode} onChange={onConversationModeChange} disabled={sending} />
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
              {modelsLoading ? (
                <div className="clone-select-option">
                  <span className="clone-select-path">Загрузка…</span>
                </div>
              ) : null}
              {activeProvider?.model && !models.some((m) => m.id === activeProvider.model) ? (
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
              {models.map((m) => (
                <button
                  key={m.id}
                  type="button"
                  role="option"
                  aria-selected={m.id === activeProvider?.model}
                  className={`clone-select-option${m.id === activeProvider?.model ? " is-active" : ""}`}
                  onClick={() => handleSelectModel(m.id)}
                >
                  <span className="clone-select-path">{m.id}</span>
                </button>
              ))}
            </div>
          ) : null}
        </div>
        {contextLimit !== null ? (
          <div
            className={`assistant-context-bar${contextUsageRatio !== null && contextUsageRatio >= CONTEXT_NEAR_LIMIT_RATIO ? " near-limit" : ""}`}
            title={`Оценка использования контекста: ~${contextTokens.toLocaleString("ru-RU")} из ${contextLimit.toLocaleString("ru-RU")} токенов`}
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
      {showFollowUpBar && followUpSuggestions ? (
        <div className="assistant-followup-bar" role="group" aria-label="Похожие предложения">
          <Sparkles className="assistant-followup-bar-icon" size={12} strokeWidth={1.75} aria-hidden />
          <div className="assistant-followup-bar-chips">
            {followUpSuggestions.map((s) => (
              <button
                key={s.id}
                type="button"
                className="assistant-followup-chip"
                disabled={sending}
                onClick={() => handleSuggestionClick(s)}
              >
                {s.label}
              </button>
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
            placeholder={`Спросите что-нибудь…\n(Enter — отправить, Shift+Enter — новая строка)`}
            disabled={sending}
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
    </>
  );
}
