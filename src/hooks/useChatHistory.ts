import { useCallback, useEffect, useRef, useState } from "react";
import type { Task } from "../lib/aiTools";
import type { ChatMessage } from "../lib/chatBlocks";
import {
  deriveChatTitle,
  listChats,
  loadChatMessages,
  saveChat,
  setChatArchived,
  type ChatSummary,
} from "../lib/chatHistory";
import type { PendingApproval } from "../lib/llm";
import { memoryExtractTurn } from "../lib/memoryPipeline";

/** Owns chat-list/switch/archive state for one repository — lives inside
 * `AssistantPanel` (not lifted to `App.tsx`): `RightDock` only mounts
 * `AssistantPanel` while the assistant tool is active, so this hook
 * naturally re-resolves "most recent chat" every time the panel is opened,
 * which is exactly the "opens the last active chat" requirement, not a
 * workaround.
 *
 * `currentChatId` is minted via `crypto.randomUUID()` immediately — on the
 * initial scan finding no chats, and on every `newChat()` — rather than
 * deferred to first save. That's what makes `currentChatId` non-null for
 * essentially the hook's whole lifetime, which in turn is what lets a
 * consumer use `key={currentChatId}` alone as a always-unique React remount
 * key for the conversation subtree (see `AssistantPanel.tsx`). */
export function useChatHistory(repoRoot: string | null) {
  const [activeChats, setActiveChats] = useState<ChatSummary[]>([]);
  const [archivedChats, setArchivedChats] = useState<ChatSummary[] | null>(null);
  const [archivedLoading, setArchivedLoading] = useState(false);
  const [currentChatId, setCurrentChatId] = useState<string | null>(null);
  const [currentMessages, setCurrentMessages] = useState<ChatMessage[] | null>(null);
  const [currentTodos, setCurrentTodos] = useState<Task[] | null>(null);
  const [currentActivePlanId, setCurrentActivePlanId] = useState<string | null>(null);
  // Set when the loaded chat was last saved mid-turn, paused awaiting a
  // tool-approval/`askUser` decision never resolved before the app closed —
  // see `savePendingApproval` below and `useLlmChat`'s cold-hydrate effect.
  const [currentPendingResume, setCurrentPendingResume] = useState<PendingApproval | null>(null);

  // Guards against a stale async load (the initial repo scan, or a
  // `switchChat`) applying its result after a newer one has already
  // started — bumped on every `repoRoot` change and every `switchChat`/
  // `newChat` call. Token-based rather than a plain `cancelled` boolean
  // (the shape `useDocsTree`/`useSpecsRepo` use) because more than one kind
  // of async operation can invalidate the same in-flight load here.
  const loadTokenRef = useRef(0);

  useEffect(() => {
    const token = ++loadTokenRef.current;
    setCurrentChatId(null);
    setCurrentMessages(null);
    setCurrentTodos(null);
    setCurrentActivePlanId(null);
    setCurrentPendingResume(null);
    setArchivedChats(null);
    if (!repoRoot) {
      setActiveChats([]);
      return;
    }
    void (async () => {
      const chats = await listChats(repoRoot, false).catch(() => []);
      if (loadTokenRef.current !== token) return;
      setActiveChats(chats);
      if (chats.length > 0) {
        const loaded = await loadChatMessages(chats[0].id).catch(() => ({
          messages: [] as ChatMessage[],
          todos: [] as Task[],
          activePlanId: null as string | null,
          pendingResume: null as PendingApproval | null,
        }));
        if (loadTokenRef.current !== token) return;
        setCurrentChatId(chats[0].id);
        setCurrentMessages(loaded.messages);
        setCurrentTodos(loaded.todos);
        setCurrentActivePlanId(loaded.activePlanId ?? null);
        setCurrentPendingResume(loaded.pendingResume ?? null);
      } else {
        setCurrentChatId(crypto.randomUUID());
        setCurrentMessages([]);
        setCurrentTodos([]);
        setCurrentActivePlanId(null);
        setCurrentPendingResume(null);
      }
    })();
  }, [repoRoot]);

  const switchChat = useCallback((chatId: string) => {
    const token = ++loadTokenRef.current;
    setCurrentChatId(chatId);
    setCurrentMessages(null);
    setCurrentTodos(null);
    setCurrentActivePlanId(null);
    setCurrentPendingResume(null);
    void loadChatMessages(chatId)
      .then((loaded) => {
        if (loadTokenRef.current === token) {
          setCurrentMessages(loaded.messages);
          setCurrentTodos(loaded.todos);
          setCurrentActivePlanId(loaded.activePlanId ?? null);
          setCurrentPendingResume(loaded.pendingResume ?? null);
        }
      })
      .catch(() => {
        if (loadTokenRef.current === token) {
          setCurrentMessages([]);
          setCurrentTodos([]);
          setCurrentActivePlanId(null);
          setCurrentPendingResume(null);
        }
      });
  }, []);

  const newChat = useCallback(() => {
    loadTokenRef.current++; // invalidate any in-flight load — nothing left to wait for
    setCurrentChatId(crypto.randomUUID());
    setCurrentMessages([]);
    setCurrentTodos([]);
    setCurrentActivePlanId(null);
    setCurrentPendingResume(null);
  }, []);

  const loadArchived = useCallback(async () => {
    if (!repoRoot) return;
    setArchivedLoading(true);
    try {
      setArchivedChats(await listChats(repoRoot, true));
    } catch {
      setArchivedChats([]);
    } finally {
      setArchivedLoading(false);
    }
  }, [repoRoot]);

  // Re-fetches the relevant list(s) from the backend rather than
  // hand-splicing local state — this data is small and local (sub-
  // millisecond SQLite), so the round trip is free, and this avoids a
  // whole class of "did I resort/merge correctly" bugs.
  const archiveChat = useCallback(
    async (chatId: string) => {
      await setChatArchived(chatId, true).catch(() => {});
      const chats = repoRoot ? await listChats(repoRoot, false).catch(() => activeChats) : [];
      setActiveChats(chats);
      if (archivedChats !== null && repoRoot) {
        setArchivedChats(await listChats(repoRoot, true).catch(() => archivedChats));
      }
      if (chatId === currentChatId) {
        if (chats.length > 0) switchChat(chats[0].id);
        else newChat();
      }
    },
    [repoRoot, currentChatId, activeChats, archivedChats, switchChat, newChat],
  );

  const unarchiveChat = useCallback(
    async (chatId: string) => {
      await setChatArchived(chatId, false).catch(() => {});
      if (!repoRoot) return;
      setActiveChats(await listChats(repoRoot, false).catch(() => activeChats));
      if (archivedChats !== null) {
        setArchivedChats(await listChats(repoRoot, true).catch(() => archivedChats));
      }
    },
    [repoRoot, activeChats, archivedChats],
  );

  // Passed straight through as `useLlmChat`'s `onTurnSettled`. Fire-and-
  // forget: a failed save only means that one turn wasn't persisted, not
  // something that should interrupt an in-progress conversation — no error
  // UI, just a console note. Explicitly clears any `pending_resume` left
  // over from an earlier pause in this same turn — the turn just settled
  // (done/cancelled/error), so nothing is left to resume.
  const saveTurn = useCallback(
    (messages: ChatMessage[], todos: Task[], activePlanId: string | null = null) => {
      if (!repoRoot || !currentChatId || messages.length === 0) return;
      const title = deriveChatTitle(messages);
      setCurrentActivePlanId(activePlanId);
      setCurrentPendingResume(null);
      void saveChat(repoRoot, currentChatId, title, messages, todos, activePlanId, null)
        .then((summary) => {
          setActiveChats((prev) => [summary, ...prev.filter((c) => c.id !== summary.id)]);
          void memoryExtractTurn(repoRoot, currentChatId).catch((e: unknown) => {
            console.error("Не удалось запустить извлечение памяти", e);
          });
        })
        .catch((e: unknown) => {
          console.error("Не удалось сохранить историю чата", e);
        });
    },
    [repoRoot, currentChatId],
  );

  // Passed straight through as `useLlmChat`'s `onTurnPaused` — fired the
  // moment a round pauses awaiting a tool-approval/`askUser` decision, not
  // just once the whole turn finally settles like `saveTurn` above. Same
  // fire-and-forget convention. Deliberately skips `memoryExtractTurn` —
  // that pipeline only makes sense for a turn that actually concluded.
  const savePendingApproval = useCallback(
    (messages: ChatMessage[], todos: Task[], activePlanId: string | null, pendingResume: PendingApproval) => {
      if (!repoRoot || !currentChatId || messages.length === 0) return;
      const title = deriveChatTitle(messages);
      setCurrentActivePlanId(activePlanId);
      setCurrentPendingResume(pendingResume);
      void saveChat(repoRoot, currentChatId, title, messages, todos, activePlanId, pendingResume)
        .then((summary) => {
          setActiveChats((prev) => [summary, ...prev.filter((c) => c.id !== summary.id)]);
        })
        .catch((e: unknown) => {
          console.error("Не удалось сохранить состояние ожидающего ответа", e);
        });
    },
    [repoRoot, currentChatId],
  );

  return {
    activeChats,
    archivedChats,
    archivedLoading,
    currentChatId,
    currentMessages,
    currentTodos,
    currentActivePlanId,
    currentPendingResume,
    switchChat,
    newChat,
    archiveChat,
    unarchiveChat,
    loadArchived,
    saveTurn,
    savePendingApproval,
  };
}
