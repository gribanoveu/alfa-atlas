import { ArrowLeft } from "lucide-react";
import type { ChatSummary } from "../../lib/chatHistory";

type ArchivedChatsPanelProps = {
  chats: ChatSummary[] | null;
  loading: boolean;
  onUnarchive: (chatId: string) => void;
  onClose: () => void;
};

/** Browse-only view of a repo's archived chats — replaces the conversation
 * area in place (not an overlay), opened solely by `ChatHistoryMenu`'s
 * dedicated archive button. Selecting a row doesn't resume it; the only
 * action is "Восстановить", after which the chat reappears in the normal
 * dropdown, switchable like any other chat once the user goes back. */
export function ArchivedChatsPanel({ chats, loading, onUnarchive, onClose }: ArchivedChatsPanelProps) {
  return (
    <div className="assistant-archive-inline">
      <div className="assistant-archive-header">
        <button type="button" className="assistant-archive-back" onClick={onClose} aria-label="Назад к чату">
          <ArrowLeft size={15} aria-hidden />
        </button>
        <span className="assistant-archive-header-title">Архив чатов</span>
      </div>

      <div className="assistant-archive-list">
        {loading ? (
          <p className="assistant-archive-empty">Загрузка…</p>
        ) : !chats || chats.length === 0 ? (
          <p className="assistant-archive-empty">В архиве пока пусто</p>
        ) : (
          chats.map((chat) => (
            <div key={chat.id} className="assistant-archive-item">
              <span className="assistant-archive-item-title">{chat.title || "Новый чат"}</span>
              <button type="button" className="assistant-btn" onClick={() => onUnarchive(chat.id)}>
                Восстановить
              </button>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
