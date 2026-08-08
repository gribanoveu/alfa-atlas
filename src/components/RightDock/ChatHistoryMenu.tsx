import { Archive, ChevronDown, Plus } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ChatSummary } from "../../lib/chatHistory";

type ChatHistoryMenuProps = {
  chats: ChatSummary[];
  currentChatId: string | null;
  /** Label shown on the trigger — `chats` may not yet contain
   * `currentChatId` (a brand-new, never-saved chat), so this is passed
   * separately rather than derived by looking it up in `chats`. */
  currentTitle: string;
  /** Disabled while a turn is in flight — see `AssistantPanel.tsx`'s doc
   * comment on why switching mid-turn isn't safe. */
  disabled: boolean;
  onSelect: (chatId: string) => void;
  onArchive: (chatId: string) => void;
  onNewChat: () => void;
  onShowArchive: () => void;
};

function formatRelativeTime(unixMillis: number): string {
  const diffMin = Math.floor((Date.now() - unixMillis) / 60_000);
  if (diffMin < 1) return "только что";
  if (diffMin < 60) return `${diffMin} мин назад`;
  const diffHours = Math.floor(diffMin / 60);
  if (diffHours < 24) return `${diffHours} ч назад`;
  const diffDays = Math.floor(diffHours / 24);
  if (diffDays < 7) return `${diffDays} дн назад`;
  return new Date(unixMillis).toLocaleDateString("ru-RU", { day: "numeric", month: "short" });
}

/** The chat-switcher row at the top of the assistant panel: a dropdown
 * listing active (non-archived) chats for the current repo — cloning the
 * model picker's `.clone-select` trigger/menu/click-outside pattern
 * verbatim — plus a "new chat" button and a "show archive" button. Each row
 * carries its own small archive action rather than requiring the user to
 * open the chat first to archive it. */
export function ChatHistoryMenu({
  chats,
  currentChatId,
  currentTitle,
  disabled,
  onSelect,
  onArchive,
  onNewChat,
  onShowArchive,
}: ChatHistoryMenuProps) {
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

  return (
    <div className="assistant-chat-switcher">
      <div className="clone-select assistant-chat-history-select" ref={ref}>
        <button
          type="button"
          className={`clone-select-trigger${open ? " is-open" : ""}`}
          aria-haspopup="listbox"
          aria-expanded={open}
          disabled={disabled}
          onClick={() => setOpen((v) => !v)}
        >
          <span className="clone-select-value">
            <span className="clone-select-path">{currentTitle}</span>
          </span>
          <ChevronDown className="clone-select-chevron" size={13} aria-hidden />
        </button>
        {open ? (
          <div className="clone-select-menu" role="listbox">
            {chats.length === 0 ? (
              <div className="clone-select-option">
                <span className="clone-select-path">Пока нет сохранённых чатов</span>
              </div>
            ) : (
              chats.map((chat) => (
                <div key={chat.id} className="assistant-chat-history-item">
                  <button
                    type="button"
                    role="option"
                    aria-selected={chat.id === currentChatId}
                    className={`clone-select-option assistant-chat-history-option${
                      chat.id === currentChatId ? " is-active" : ""
                    }`}
                    onClick={() => {
                      setOpen(false);
                      onSelect(chat.id);
                    }}
                  >
                    <span className="clone-select-path">{chat.title || "Новый чат"}</span>
                    <span className="assistant-chat-history-time">{formatRelativeTime(chat.updatedAt)}</span>
                  </button>
                  <button
                    type="button"
                    className="assistant-chat-history-archive"
                    aria-label={`Архивировать «${chat.title || "Новый чат"}»`}
                    title="Архивировать"
                    onClick={(event) => {
                      event.stopPropagation();
                      onArchive(chat.id);
                    }}
                  >
                    <Archive size={13} aria-hidden />
                  </button>
                </div>
              ))
            )}
          </div>
        ) : null}
      </div>
      <button
        type="button"
        className="assistant-btn assistant-chat-icon-btn"
        disabled={disabled}
        title="Новый чат"
        aria-label="Новый чат"
        onClick={onNewChat}
      >
        <Plus size={18} strokeWidth={2} aria-hidden />
      </button>
      <button
        type="button"
        className="assistant-btn assistant-chat-icon-btn"
        disabled={disabled}
        title="Архив чатов"
        aria-label="Архив чатов"
        onClick={onShowArchive}
      >
        <Archive size={17} strokeWidth={1.9} aria-hidden />
      </button>
    </div>
  );
}
