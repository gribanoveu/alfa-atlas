import { useEffect, useRef, useState } from "react";
import { FileText, FolderGit2, Send, Settings2, Sparkles } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useAiAccessMode } from "../../hooks/useAiAccessMode";
import { useEmbeddingSetup } from "../../hooks/useEmbeddingSetup";
import { useLlmChat } from "../../hooks/useLlmChat";
import { useLlmSetup } from "../../hooks/useLlmSetup";
import {
  AUTO_MODEL_LABEL,
  AUTO_MODEL_VALUE,
  CHAT_INPUT_ROWS,
  CONTEXT_NEAR_LIMIT_RATIO,
} from "../../lib/assistantConfig";
import type { AiAccessMode } from "../../lib/aiTools";
import type { LlmModelInfo } from "../../lib/llm";
import { AssistantMarkdown } from "./AssistantMarkdown";
import "../Welcome/CloneRepoModal.css";
import "./AssistantPanel.css";

const ACCESS_MODE_OPTIONS: { value: AiAccessMode; label: string; Icon: LucideIcon }[] = [
  { value: "docsOnly", label: "Документация", Icon: FileText },
  { value: "fullRepo", label: "Весь репозиторий", Icon: FolderGit2 },
];

function trimTrailingZero(n: number): string {
  return Number.isInteger(n) ? String(n) : n.toFixed(1);
}

function formatTokenCount(n: number): string {
  if (n >= 1_000_000) return `${trimTrailingZero(n / 1_000_000)}M`;
  if (n >= 1_000) return `${trimTrailingZero(n / 1_000)}K`;
  return String(n);
}

type AssistantPanelProps = {
  onOpenSettings: () => void;
};

/** This panel is the assistant's actual interaction surface — plain,
 * streamed conversation with the configured LLM provider (no tool-calling/
 * search yet, see AI_HARNESS.md's "not built yet" section). Below the
 * access-mode toggle (the one control that's genuinely specific to talking
 * to the assistant, not to indexing) this renders exactly one of two
 * states: a compact setup prompt (only when no LLM provider is ready — an
 * active/first provider with a saved API key), or the chat surface.
 *
 * The **embedding** provider/index being incomplete is deliberately *not*
 * a gate here: plain chat doesn't touch embeddings at all yet, and once
 * tool-calling exists `ReadFile`/`ListFiles`/the lexical/symbol tiers of
 * `services::ai_tools::semantic_search` already work with zero embeddings
 * — so its readiness is surfaced only as a non-blocking info note.
 */
export function AssistantPanel({ onOpenSettings }: AssistantPanelProps) {
  const {
    providerConfigured: embeddingConfigured,
    indexStatus,
    lastSync,
    syncProgress,
    busy,
    sync,
  } = useEmbeddingSetup();
  const { mode: accessMode, busy: accessModeBusy, setMode: setAccessMode } = useAiAccessMode();
  const { settings, providers, hasApiKeyMap, updateProviderConfig, loadModels } = useLlmSetup();

  const activeProviderId = settings?.activeProviderId ?? providers[0]?.id ?? null;
  const activeProvider = providers.find((p) => p.id === activeProviderId) ?? null;
  const llmReady = activeProviderId !== null && Boolean(hasApiKeyMap[activeProviderId]);
  // `accessMode` is `null` only until the first `getAiAccessMode` round trip
  // resolves; "docsOnly" is the same safe default the backend itself falls
  // back to (`AiAccessMode::default()`), so the very first system prompt
  // built before that resolves is never wrong about being unrestricted.
  const { messages, sending, error, sendMessage, contextTokens } = useLlmChat(
    activeProviderId,
    accessMode ?? "docsOnly",
  );
  const contextLimit = activeProvider?.limit?.context ?? null;
  const contextUsageRatio = contextLimit ? Math.min(1, contextTokens / contextLimit) : null;
  const [draft, setDraft] = useState("");

  const messagesRef = useRef<HTMLDivElement>(null);

  // Model picker — reuses the same `.clone-select*` trigger/menu pattern as
  // `LlmTab.tsx`'s own model dropdown (and writes to the same underlying
  // setting via `updateProviderConfig`), so picking a model here or in
  // Settings stays a single source of truth.
  const [models, setModels] = useState<LlmModelInfo[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelSelectOpen, setModelSelectOpen] = useState(false);
  const modelSelectRef = useRef<HTMLDivElement>(null);

  // One index now covers the whole repository regardless of `accessMode`
  // (see `resolve_index_paths` in `commands/embeddings.rs`) — the mode only
  // changes what the AI assistant is allowed to read/search, not what's
  // indexed, so switching it no longer needs to re-fetch `indexStatus`.
  const handleAccessModeChange = (value: AiAccessMode) => {
    void setAccessMode(value);
  };

  // `lastSync` (this session's own sync) counts as ready immediately, same
  // as before — `indexStatus.synced` alone would lag by one round trip
  // right after a sync finishes, until the next `embedding_index_status`
  // refetch.
  const indexReady = Boolean(indexStatus?.synced) || lastSync !== null;

  // Fires once per mount rather than requiring the user to click
  // "Синхронизировать" — `embedding_sync`'s own hash comparison makes a
  // redundant call cheap, but there's no reason to re-trigger on every
  // render, and `indexReady` flips true as soon as *anything* is embedded
  // (not full completeness), so in practice this only ever does real work
  // the first time a project has never been synced.
  const autoSyncTriggered = useRef(false);
  useEffect(() => {
    if (!embeddingConfigured || indexReady || busy || autoSyncTriggered.current) return;
    autoSyncTriggered.current = true;
    void sync();
  }, [embeddingConfigured, indexReady, busy, sync]);

  useEffect(() => {
    const el = messagesRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages]);

  // Reset the fetched model list (and close the menu) whenever the active
  // provider itself changes, so a stale list from a different provider
  // never briefly shows.
  useEffect(() => {
    setModels([]);
    setModelSelectOpen(false);
  }, [activeProviderId]);

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
      if (next && models.length === 0 && !modelsLoading && activeProviderId) {
        setModelsLoading(true);
        loadModels(activeProviderId)
          .then(setModels)
          .catch(() => {})
          .finally(() => setModelsLoading(false));
      }
      return next;
    });
  };

  const handleSelectModel = (value: string) => {
    if (!activeProviderId) return;
    setModelSelectOpen(false);
    void updateProviderConfig(activeProviderId, { model: value === AUTO_MODEL_VALUE ? null : value });
  };

  const handleSend = () => {
    const text = draft.trim();
    if (!text || !llmReady || sending) return;
    setDraft("");
    void sendMessage(text);
  };

  return (
    <div className="assistant-panel">
      <section className="assistant-panel-access">
        <div className="assistant-access-toggle" role="radiogroup" aria-label="Область доступа AI">
          {ACCESS_MODE_OPTIONS.map((option) => (
            <button
              key={option.value}
              type="button"
              role="radio"
              aria-checked={accessMode === option.value}
              className={`assistant-access-btn ${accessMode === option.value ? "active" : ""}`}
              disabled={accessModeBusy || accessMode === null}
              onClick={() => handleAccessModeChange(option.value)}
            >
              <option.Icon size={13} strokeWidth={1.75} aria-hidden />
              {option.label}
            </button>
          ))}
        </div>
      </section>

      <div className="assistant-chat">
        {llmReady ? (
          <>
            <div className="assistant-model-bar">
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
            </div>

            {!embeddingConfigured ? (
              <p className="assistant-chat-index-note">
                Провайдер эмбеддингов не настроен — поиск по документации будет ограничен. Настройте
                его в Настройки → Эмбеддинги.
              </p>
            ) : !indexReady ? (
              <p className="assistant-chat-index-note">
                {busy && syncProgress
                  ? `Строится индекс документации: ${syncProgress.current}/${syncProgress.total}…`
                  : "Индекс документации ещё строится — ответы будут менее точными, пока индексация не завершится."}
              </p>
            ) : null}
            <div className="assistant-chat-messages" ref={messagesRef}>
              {messages.length === 0 ? (
                <div className="assistant-chat-placeholder">
                  <Sparkles size={22} strokeWidth={1.5} aria-hidden />
                  <p className="assistant-chat-placeholder-title">Ассистент готов</p>
                  <p className="assistant-chat-placeholder-desc">Задайте вопрос о документации проекта.</p>
                </div>
              ) : (
                messages.map((m) => (
                  <div key={m.id} className={`assistant-chat-message ${m.role}${m.failed ? " failed" : ""}`}>
                    {m.role === "assistant" && m.streaming && m.content === "" ? (
                      <span className="assistant-chat-typing" aria-label="Ассистент печатает…">
                        <span />
                        <span />
                        <span />
                      </span>
                    ) : m.role === "assistant" ? (
                      <AssistantMarkdown content={m.content} streaming={Boolean(m.streaming)} />
                    ) : (
                      m.content
                    )}
                  </div>
                ))
              )}
            </div>
            {contextLimit !== null ? (
              <div
                className={`assistant-context-bar${contextUsageRatio !== null && contextUsageRatio >= CONTEXT_NEAR_LIMIT_RATIO ? " near-limit" : ""}`}
                title={`Оценка использования контекста: ~${contextTokens.toLocaleString("ru-RU")} из ${contextLimit.toLocaleString("ru-RU")} токенов`}
              >
                <div className="assistant-context-bar-track">
                  <div
                    className="assistant-context-bar-fill"
                    style={{ width: `${(contextUsageRatio ?? 0) * 100}%` }}
                  />
                </div>
                <span className="assistant-context-bar-label">
                  {formatTokenCount(contextTokens)} / {formatTokenCount(contextLimit)}
                </span>
              </div>
            ) : null}
            {error ? <div className="assistant-chat-error">{error}</div> : null}
            <div className="assistant-chat-input-row">
              <div className="assistant-chat-input-wrap">
                <textarea
                  className="assistant-chat-input"
                  rows={CHAT_INPUT_ROWS}
                  value={draft}
                  placeholder={`Спросите что-нибудь…\n(Enter — отправить, Shift+Enter — новая строка)`}
                  disabled={!llmReady || sending}
                  onChange={(event) => setDraft(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && !event.shiftKey) {
                      event.preventDefault();
                      handleSend();
                    }
                  }}
                />
                <button
                  type="button"
                  className="assistant-chat-send"
                  disabled={!llmReady || sending || !draft.trim()}
                  aria-label="Отправить"
                  onClick={handleSend}
                >
                  <Send size={15} strokeWidth={1.75} aria-hidden />
                </button>
              </div>
            </div>
          </>
        ) : (
          <div className="assistant-setup-prompt">
            <Settings2 size={22} strokeWidth={1.5} aria-hidden />
            <p className="assistant-setup-title">Провайдер LLM не настроен</p>
            <p className="assistant-setup-desc">
              Чтобы начать общение с ассистентом, настройте провайдера LLM и сохраните API-ключ.
            </p>
            <button type="button" className="assistant-btn primary" onClick={onOpenSettings}>
              Открыть настройки
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
