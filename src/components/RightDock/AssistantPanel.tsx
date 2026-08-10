import { useEffect, useMemo, useRef, useState } from "react";
import { FileText, FolderGit2, Settings2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useAiAccessMode } from "../../hooks/useAiAccessMode";
import { useChatHistory } from "../../hooks/useChatHistory";
import { useEmbeddingSetup } from "../../hooks/useEmbeddingSetup";
import { useLlmSetup } from "../../hooks/useLlmSetup";
import { useToolDefinitions } from "../../hooks/useToolDefinitions";
import type { AiAccessMode } from "../../lib/aiTools";
import { deriveChatTitle } from "../../lib/chatHistory";
import type { SpecsRepoInfo } from "../../lib/openapi";
import type { UpdatedReference } from "../../lib/project";
import { docsRootRelativeToRepo } from "../../lib/paths";
import { ArchivedChatsPanel } from "./ArchivedChatsPanel";
import { AssistantConversation } from "./AssistantConversation";
import { ChatHistoryMenu } from "./ChatHistoryMenu";
import "../Welcome/CloneRepoModal.css";
import "./AssistantPanel.css";

const ACCESS_MODE_OPTIONS: { value: AiAccessMode; label: string; Icon: LucideIcon }[] = [
  { value: "docsOnly", label: "Документация", Icon: FileText },
  { value: "fullRepo", label: "Весь репозиторий", Icon: FolderGit2 },
];

type AssistantPanelProps = {
  onOpenSettings: () => void;
  /** `useSpecsRepo`'s detection result for the open project (`App.tsx`
   * already runs it once per `repoRoot`) — forwarded into the system
   * prompt's "Current project type" line, see `buildAssistantSystemPrompt`. */
  specsRepoInfo: SpecsRepoInfo | null;
  /** The open project's docs root — needed by a `"pendingApproval"` card's
   * `writeFile` diff preview (`AssistantToolCallBlock`) to fetch a file's
   * current content for the original/proposed comparison. */
  docsRoot: string;
  /** Called once a `writeFile`/`editFile`/`deleteFile`/`createDirectory`/
   * `deleteDirectory` tool call actually lands on disk (its block settles
   * to `"done"`) — `App.tsx`'s own `useDocsTree` and `useEditorTabs`
   * instances live outside this component, so a successful assistant-driven
   * change needs this callback both to make the change show up in the
   * sidebar tree (the same way every UI-driven file operation already calls
   * `tree.refresh()` itself after its backend command resolves) and to
   * reconcile any open editor tab for `path` — otherwise a stale tab's
   * autosave would silently overwrite the assistant's change right back. */
  onFileWritten: (info: { tool: string; path: string }) => void;
  /** Called once a `move` tool call actually lands on disk — separate from
   * `onFileWritten` because a move carries both an old and a new path (plus
   * a `RenameReport` of cascaded reference rewrites), not the single `path`
   * every other mutating tool settles with. `App.tsx`'s handler remaps any
   * open editor tab from `from` to `to` (`editor.remapTabsUnder`), the same
   * way the manual drag-and-drop move already does. */
  onFileMoved: (info: { from: string; to: string; updatedFiles: UpdatedReference[] }) => void;
  /** The open project's repo root — keys persisted chat history
   * (`useChatHistory`) per repository. */
  repoRoot: string | null;
};

/** This panel is the assistant's actual interaction surface. It owns
 * project-wide concerns (LLM/embedding readiness, the access-mode toggle,
 * which chat is active) and delegates one conversation's actual streamed
 * exchange to `AssistantConversation`, rendered with `key={currentChatId}`
 * so switching chats is a clean React remount — that's what resets
 * `useLlmChat`'s internal refs (per-tool trust set, in-flight approval
 * timers) correctly, without a manual reset effect that could race an
 * in-flight turn.
 *
 * Chat-switching/new-chat are disabled while a turn is in flight
 * (`conversationSending`, bubbled up from `AssistantConversation`) — not
 * just UX polish: `CHAT_STREAM_DELTA_EVENT`/`TOOL_CALL_EVENT`/
 * `TOOL_RESULT_EVENT` are global, unscoped Tauri events (`commands::llm`'s
 * own doc comment: "this app has exactly one chat panel / one in-flight
 * conversation at a time"). Allowing a remount mid-turn would let a
 * still-in-flight call's late-arriving events land on the new chat's fresh
 * `useLlmChat` instance, with no id to filter on.
 *
 * The **embedding** provider/index being incomplete is deliberately not a
 * gate on the chat surface itself — the lexical/symbol tiers of
 * `services::ai_tools::semantic_search` already work with zero embeddings —
 * so its readiness is surfaced only as a non-blocking info note above the
 * transcript. */
export function AssistantPanel({
  onOpenSettings,
  specsRepoInfo,
  docsRoot,
  onFileWritten,
  onFileMoved,
  repoRoot,
}: AssistantPanelProps) {
  const {
    providerConfigured: embeddingConfigured,
    indexStatus,
    lastSync,
    syncProgress,
    busy,
    sync,
  } = useEmbeddingSetup();
  const {
    mode: accessMode,
    busy: accessModeBusy,
    setMode: setAccessMode,
    refresh: refreshAccessMode,
  } = useAiAccessMode();
  const { settings, providers, hasApiKeyMap, updateProviderConfig, loadModels } = useLlmSetup();
  const { definitions: toolDefinitions } = useToolDefinitions(accessMode ?? "docsOnly");

  const activeProviderId = settings?.activeProviderId ?? providers[0]?.id ?? null;
  const activeProvider = providers.find((p) => p.id === activeProviderId) ?? null;
  const llmReady = activeProviderId !== null && Boolean(hasApiKeyMap[activeProviderId]);

  const chatHistory = useChatHistory(repoRoot);
  // The documentation root's path relative to the repository root (e.g.
  // `"src/docs/asciidoc"`), or `null` when the distinction doesn't matter —
  // fed into the system prompt so it states the real Full-repo-mode path
  // prefix instead of a generic illustrative example. See
  // `docsRootRelativeToRepo`'s own doc comment.
  const docsRootPrefix = useMemo(
    () => docsRootRelativeToRepo(repoRoot ?? "", docsRoot),
    [repoRoot, docsRoot],
  );
  const [conversationSending, setConversationSending] = useState(false);
  const [archiveOpen, setArchiveOpen] = useState(false);

  // The active chat's saved title, or — for a chat that hasn't completed
  // its first turn yet (so it isn't in `activeChats`) — a live preview
  // derived from whatever's been typed so far, same rule `saveTurn` itself
  // will use once it does save.
  const currentSummary = chatHistory.activeChats.find((c) => c.id === chatHistory.currentChatId);
  const currentTitle = currentSummary?.title || deriveChatTitle(chatHistory.currentMessages ?? []);

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

  const handleShowArchive = () => {
    setArchiveOpen(true);
    void chatHistory.loadArchived();
  };

  // Switching or starting a chat while browsing the archive should bring
  // the conversation back into view — the archive is a view within this
  // same panel now (not a separate overlay), so leaving it has to be
  // implicit in these actions, not just the archive's own back button.
  const handleSelectChat = (chatId: string) => {
    setArchiveOpen(false);
    chatHistory.switchChat(chatId);
  };

  const handleNewChat = () => {
    setArchiveOpen(false);
    chatHistory.newChat();
  };

  return (
    <div className="assistant-panel">
      {llmReady ? (
        <ChatHistoryMenu
          chats={chatHistory.activeChats}
          currentChatId={chatHistory.currentChatId}
          currentTitle={currentTitle}
          disabled={conversationSending}
          onSelect={handleSelectChat}
          onArchive={(id) => void chatHistory.archiveChat(id)}
          onNewChat={handleNewChat}
          onShowArchive={handleShowArchive}
        />
      ) : null}

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
        {archiveOpen ? (
          <ArchivedChatsPanel
            chats={chatHistory.archivedChats}
            loading={chatHistory.archivedLoading}
            onUnarchive={(id) => void chatHistory.unarchiveChat(id)}
            onClose={() => setArchiveOpen(false)}
          />
        ) : llmReady ? (
          <>
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

            {chatHistory.currentChatId === null ||
            chatHistory.currentMessages === null ||
            chatHistory.currentTodos === null ? (
              <div className="assistant-chat-placeholder">
                <p className="assistant-chat-placeholder-desc">Загрузка истории чата…</p>
              </div>
            ) : (
              <AssistantConversation
                key={chatHistory.currentChatId}
                initialMessages={chatHistory.currentMessages}
                initialTodos={chatHistory.currentTodos}
                onTurnSettled={chatHistory.saveTurn}
                onSendingChange={setConversationSending}
                providerId={activeProviderId}
                accessMode={accessMode ?? "docsOnly"}
                specsRepoInfo={specsRepoInfo}
                toolDefinitions={toolDefinitions}
                docsRootRelativeToRepo={docsRootPrefix}
                docsRoot={docsRoot}
                onFileWritten={onFileWritten}
                onFileMoved={onFileMoved}
                refreshAccessMode={refreshAccessMode}
                activeProvider={activeProvider}
                updateProviderConfig={updateProviderConfig}
                loadModels={loadModels}
              />
            )}
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
