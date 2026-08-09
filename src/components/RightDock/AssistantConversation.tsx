import { useEffect, useRef, useState } from "react";
import { ArrowDown, ChevronUp, Send, Sparkles } from "lucide-react";
import { useLlmChat } from "../../hooks/useLlmChat";
import {
  AUTO_MODEL_LABEL,
  AUTO_MODEL_VALUE,
  CHAT_INPUT_ROWS,
  CONTEXT_NEAR_LIMIT_RATIO,
} from "../../lib/assistantConfig";
import type { AiAccessMode, LlmToolDefinition } from "../../lib/aiTools";
import type { ChatMessage } from "../../lib/chatBlocks";
import type { LlmModelInfo, LlmProviderConfig, ResolvedLlmProvider } from "../../lib/llm";
import type { SpecsRepoInfo } from "../../lib/openapi";
import type { UpdatedReference } from "../../lib/project";
import { AssistantMarkdown } from "./AssistantMarkdown";
import { AssistantToolCallBlock } from "./AssistantToolCallBlock";
import { TodoProgressWidget } from "./TodoProgressWidget";

type ChatMode = "agent" | "plan" | "question";

const CHAT_MODE_OPTIONS: { value: ChatMode; label: string }[] = [
  { value: "agent", label: "Агент" },
  { value: "plan", label: "План" },
  { value: "question", label: "Вопрос" },
];

/** Purely visual mode picker for the chat composer — selecting an option
 * doesn't change how a message is sent yet, nothing downstream reads
 * `mode`. Placeholder for future agent/plan/question behaviors; the
 * "скоро" badge in the menu makes that explicit to the user. */
function ChatModeSelect() {
  const [mode, setMode] = useState<ChatMode>("agent");
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

  const activeLabel = CHAT_MODE_OPTIONS.find((o) => o.value === mode)?.label ?? "";

  return (
    <div className="assistant-mode-select" ref={ref}>
      <button
        type="button"
        className={`assistant-mode-trigger${open ? " is-open" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        title="Режим ассистента (пока не влияет на поведение)"
        onClick={() => setOpen((v) => !v)}
      >
        <span>{activeLabel}</span>
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
              className={`assistant-mode-option${mode === option.value ? " is-active" : ""}`}
              onClick={() => {
                setMode(option.value);
                setOpen(false);
              }}
            >
              <span>{option.label}</span>
              <span className="assistant-mode-option-soon">скоро</span>
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
  onTurnSettled: (messages: ChatMessage[]) => void;
  /** Bubbles `useLlmChat`'s `sending` up to the parent, which uses it to
   * disable chat-switching/new-chat while a turn (including a
   * `pendingApproval` pause) is in flight — see `AssistantPanel.tsx`'s own
   * doc comment on why that gate is load-bearing, not just UX polish. */
  onSendingChange: (sending: boolean) => void;
  providerId: string | null;
  accessMode: AiAccessMode;
  specsRepoInfo: SpecsRepoInfo | null;
  toolDefinitions: LlmToolDefinition[];
  /** The documentation root's path relative to the repository root (e.g.
   * `"src/docs/asciidoc"`), or `null` when the distinction doesn't matter —
   * see `docsRootRelativeToRepo` in `lib/paths.ts`. Forwarded into
   * `useLlmChat` so the system prompt states the real Full-repo-mode path
   * prefix instead of a generic illustrative example. */
  docsRootRelativeToRepo: string | null;
  docsRoot: string;
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
  onTurnSettled,
  onSendingChange,
  providerId,
  accessMode,
  specsRepoInfo,
  toolDefinitions,
  docsRootRelativeToRepo,
  docsRoot,
  onFileWritten,
  onFileMoved,
  refreshAccessMode,
  activeProvider,
  updateProviderConfig,
  loadModels,
}: AssistantConversationProps) {
  const { messages, sending, error, sendMessage, contextTokens, decideToolCall, todos } = useLlmChat(
    providerId,
    accessMode,
    specsRepoInfo,
    toolDefinitions,
    docsRootRelativeToRepo,
    initialMessages,
    onTurnSettled,
  );

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
  const handledAccessGrantIdsRef = useRef<Set<string>>(new Set());
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
  }, [messages, refreshAccessMode, onFileWritten, onFileMoved]);

  const contextLimit = activeProvider?.limit?.context ?? null;
  const contextUsageRatio = contextLimit ? Math.min(1, contextTokens / contextLimit) : null;
  const [draft, setDraft] = useState("");

  const messagesRef = useRef<HTMLDivElement>(null);
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

  const handleSend = () => {
    const text = draft.trim();
    if (!text || sending) return;
    setDraft("");
    // Sending a message always means "follow the reply", even if the user
    // had scrolled up to reread something earlier in the transcript.
    pinnedToBottomRef.current = true;
    setShowJumpToBottom(false);
    void sendMessage(text);
  };

  return (
    <>
      <TodoProgressWidget tasks={todos} />
      <div className="assistant-chat-messages" ref={messagesRef} onScroll={handleMessagesScroll}>
        {messages.length === 0 ? (
          <div className="assistant-chat-placeholder">
            <Sparkles size={22} strokeWidth={1.5} aria-hidden />
            <p className="assistant-chat-placeholder-title">Ассистент готов</p>
            <p className="assistant-chat-placeholder-desc">Задайте вопрос о документации проекта.</p>
          </div>
        ) : (
          messages.map((m) => {
            const failed = m.role === "assistant" && Boolean(m.failed);
            return (
              <div key={m.id} className={`assistant-chat-message ${m.role}${failed ? " failed" : ""}`}>
                {m.role === "assistant" ? (
                  m.blocks.length === 0 && m.streaming ? (
                    <span className="assistant-chat-typing" aria-label="Ассистент печатает…">
                      <span />
                      <span />
                      <span />
                    </span>
                  ) : (
                    <div className="assistant-chat-blocks">
                      {m.blocks.map((block, i) =>
                        block.type === "text" ? (
                          <AssistantMarkdown
                            key={block.id}
                            content={block.content}
                            streaming={Boolean(m.streaming) && i === m.blocks.length - 1}
                          />
                        ) : (
                          <AssistantToolCallBlock
                            key={block.id}
                            block={block}
                            docsRoot={docsRoot}
                            onDecide={decideToolCall}
                          />
                        ),
                      )}
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
      {error ? <div className="assistant-chat-error">{error}</div> : null}
      <div className="assistant-model-bar">
        <ChatModeSelect />
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
      <div className="assistant-chat-input-row">
        <div className="assistant-chat-input-wrap">
          <textarea
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
            <button
              type="button"
              className="assistant-chat-send"
              disabled={sending || !draft.trim()}
              aria-label="Отправить"
              onClick={handleSend}
            >
              <Send size={15} strokeWidth={1.75} aria-hidden />
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
