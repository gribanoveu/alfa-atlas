import { useEffect, useRef } from "react";
import { FileText, FolderGit2, Send, Settings2, Sparkles } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useAiAccessMode } from "../../hooks/useAiAccessMode";
import { useEmbeddingSetup } from "../../hooks/useEmbeddingSetup";
import type { AiAccessMode } from "../../lib/aiTools";
import "./AssistantPanel.css";

const ACCESS_MODE_OPTIONS: { value: AiAccessMode; label: string; Icon: LucideIcon }[] = [
  { value: "docsOnly", label: "Документация", Icon: FileText },
  { value: "fullRepo", label: "Весь репозиторий", Icon: FolderGit2 },
];

type AssistantPanelProps = {
  onOpenSettings: () => void;
};

/** This panel is the assistant's actual interaction surface (chat), not a
 * setup wizard — provider/model/index setup already has a full UI in
 * Settings → Эмбеддинги (`EmbeddingsTab`); duplicating it here as a
 * checklist (the previous design) just ate the space chat needs. Below the
 * access-mode toggle (the one control that's genuinely specific to talking
 * to the assistant, not to indexing) this renders exactly one of two
 * states: a compact setup prompt (only when the embedding provider itself
 * isn't configured — there's genuinely nothing to fall back to without
 * one), or the chat surface.
 *
 * The index being incomplete is deliberately *not* a second gate: `ReadFile`/
 * `ListFiles` and the lexical/symbol tiers of `services::ai_tools::
 * semantic_search` all work with zero embeddings, and `embedding_sync`
 * already prioritizes documentation and fills in the rest in the
 * background — so the assistant is meant to get smarter as indexing
 * progresses, not sit blocked behind it. Indexing itself starts
 * automatically (see the effect below) instead of waiting on a manual
 * "Синхронизировать" click.
 *
 * No LLM is wired up yet (see AI_HARNESS.md's "not built yet" section), so
 * the chat input stays a static, disabled shell regardless of index
 * readiness — this establishes the panel's final layout now so wiring in
 * real messages later doesn't require another restructure.
 */
export function AssistantPanel({ onOpenSettings }: AssistantPanelProps) {
  const { providerConfigured, indexStatus, lastSync, syncProgress, busy, sync } =
    useEmbeddingSetup();
  const { mode: accessMode, busy: accessModeBusy, setMode: setAccessMode } = useAiAccessMode();

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
    if (!providerConfigured || indexReady || busy || autoSyncTriggered.current) return;
    autoSyncTriggered.current = true;
    void sync();
  }, [providerConfigured, indexReady, busy, sync]);

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
        <p className="assistant-access-hint">
          {accessMode === "fullRepo"
            ? "Ассистент видит весь репозиторий, включая исходный код."
            : "Ассистент видит только папку документации."}
        </p>
      </section>

      <div className="assistant-chat">
        {providerConfigured ? (
          <>
            {!indexReady ? (
              <p className="assistant-chat-index-note">
                {busy && syncProgress
                  ? `Строится индекс документации: ${syncProgress.current}/${syncProgress.total}…`
                  : "Индекс документации ещё строится — ответы будут менее точными, пока индексация не завершится."}
              </p>
            ) : null}
            <div className="assistant-chat-messages">
              <div className="assistant-chat-placeholder">
                <Sparkles size={22} strokeWidth={1.5} aria-hidden />
                <p className="assistant-chat-placeholder-title">Ассистент готов</p>
                <p className="assistant-chat-placeholder-desc">
                  Общение с ассистентом появится здесь в одном из следующих обновлений.
                </p>
              </div>
            </div>
            <div className="assistant-chat-input-row">
              <input
                className="assistant-chat-input"
                type="text"
                placeholder="Чат скоро будет доступен…"
                disabled
              />
              <button type="button" className="assistant-chat-send" disabled aria-label="Отправить">
                <Send size={15} strokeWidth={1.75} aria-hidden />
              </button>
            </div>
          </>
        ) : (
          <div className="assistant-setup-prompt">
            <Settings2 size={22} strokeWidth={1.5} aria-hidden />
            <p className="assistant-setup-title">Провайдер эмбеддингов не настроен</p>
            <p className="assistant-setup-desc">
              Чтобы включить ассистента, настройте провайдера эмбеддингов.
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
