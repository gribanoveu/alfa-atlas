import { invoke } from "@tauri-apps/api/core";
import type { Task } from "./aiTools";
import type { ChatMessage } from "./chatBlocks";
import type { PendingApproval } from "./llm";

// Mirrors `domain::chat::ChatSummary` in `src-tauri/src/domain/chat.rs`
// (`#[serde(rename_all = "camelCase")]`). `createdAt`/`updatedAt` are unix
// milliseconds.
export type ChatSummary = {
  id: string;
  repoRoot: string;
  title: string;
  archived: boolean;
  createdAt: number;
  updatedAt: number;
};

/** Active or archived chats for one repository, most recently updated
 * first (`services::chat_store::list_chats`'s `ORDER BY updated_at DESC`). */
export function listChats(repoRoot: string, archived: boolean): Promise<ChatSummary[]> {
  return invoke<ChatSummary[]>("chat_list", { repoRoot, archived });
}

// Mirrors `domain::chat::LoadedChat`. `messages` trusts the stored blob's
// shape at runtime — same trust boundary every other `invoke<T>()` call in
// this codebase already has; the backend never inspects a message's
// internals (see `infra::chat_store`'s module doc), it only stores/returns
// whatever JSON `saveChat` last wrote. `todos`, unlike `messages`, is a
// real shared type (`Task`), not an opaque blob.
export type LoadedChat = {
  messages: ChatMessage[];
  todos: Task[];
  activePlanId: string | null;
  /** Set when this chat was last saved mid-turn, paused awaiting a
   * tool-approval/`askUser` decision that was never resolved before the app
   * closed — lets `useLlmChat` resume the turn via `streamLlmChatResume`
   * after a full app restart, not just a same-session panel close. `null`
   * for a chat with no unresolved pause. */
  pendingResume: PendingApproval | null;
};

/** One chat's full state — messages (save order) and its todo checklist —
 * in one round trip, since every caller needs both together. */
export function loadChatMessages(chatId: string): Promise<LoadedChat> {
  return invoke<LoadedChat>("chat_load_messages", { chatId });
}

/** Upserts the chat row (title/todos/recency) and replaces its messages
 * wholesale — call with the conversation's full current `ChatMessage[]`
 * and `Task[]` any time it should be persisted, not incremental deltas.
 * `pendingResume` should be the raw `PendingApproval` when saving mid-turn
 * at a pause (see `useLlmChat`'s `persistPendingPause`), and `null` — the
 * default — to clear it once a turn fully settles. */
export function saveChat(
  repoRoot: string,
  chatId: string,
  title: string,
  messages: ChatMessage[],
  todos: Task[],
  activePlanId: string | null = null,
  pendingResume: PendingApproval | null = null,
): Promise<ChatSummary> {
  return invoke<ChatSummary>("chat_save", {
    repoRoot,
    chatId,
    title,
    messages,
    todos,
    activePlanId,
    pendingResume,
  });
}

export function setChatArchived(chatId: string, archived: boolean): Promise<void> {
  return invoke("chat_set_archived", { chatId, archived });
}

const TITLE_MAX_CHARS = 50;

/** Derives a chat's display title from its first user message — no shared
 * text-truncation utility exists in this codebase (see `truncateForDisplay`
 * in `AssistantToolCallBlock.tsx`, a module-private one-off doing the same
 * small thing), so this follows that convention rather than inventing a
 * shared `lib` helper. Recomputed on every save — idempotent, cheap, always
 * derives from the same first message once one exists. */
export function deriveChatTitle(messages: ChatMessage[]): string {
  const first = messages.find((m) => m.role === "user");
  if (!first || first.role !== "user") return "Новый чат";
  const trimmed = first.content.trim().replace(/\s+/g, " ");
  if (trimmed === "") return "Новый чат";
  return trimmed.length > TITLE_MAX_CHARS ? `${trimmed.slice(0, TITLE_MAX_CHARS)}…` : trimmed;
}
