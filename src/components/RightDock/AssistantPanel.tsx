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
 * states: a single compact setup prompt pointing at whatever's missing, or
 * — once ready — the chat surface. No LLM is wired up yet (see
 * AI_HARNESS.md's "not built yet" section), so the chat state today is a
 * static, disabled shell: it establishes the panel's final layout now so
 * wiring in real messages later doesn't require another restructure.
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
  const ready = providerConfigured && indexReady;

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
            ? "AI-ассистент видит весь репозиторий, включая исходный код."
            : "AI-ассистент видит только папку документации."}
        </p>
      </section>

      <div className="assistant-chat">
        {ready ? (
          <>
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
            <p className="assistant-setup-title">
              {!providerConfigured ? "Провайдер эмбеддингов не настроен" : "Индекс ещё не синхронизирован"}
            </p>
            <p className="assistant-setup-desc">
              {!providerConfigured
                ? "Чтобы включить ассистента, настройте провайдера эмбеддингов."
                : "Чтобы ассистент мог отвечать по репозиторию, синхронизируйте индекс — документация индексируется в первую очередь."}
            </p>
            {!providerConfigured ? (
              <button type="button" className="assistant-btn primary" onClick={onOpenSettings}>
                Открыть настройки
              </button>
            ) : (
              <button
                type="button"
                className="assistant-btn primary"
                disabled={busy}
                onClick={() => void sync()}
              >
                {busy
                  ? syncProgress
                    ? `${syncProgress.current}/${syncProgress.total}`
                    : "Синхронизация…"
                  : "Синхронизировать"}
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
