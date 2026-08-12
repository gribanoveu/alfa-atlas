import { invoke } from "@tauri-apps/api/core";
import type { Task } from "./aiTools";
import type { ChatMessage, MessageBlock } from "./chatBlocks";

export type ChatExportFormat = "markdown" | "json";

const INVALID_FILENAME_CHARS = /[/\\:*?"<>|\x00-\x1f]/g;

/** Turns a chat title into a filesystem-safe default filename (no
 * extension) — strips characters invalid on Windows/macOS/Linux, collapses
 * whitespace, and appends today's date so repeated exports of the same chat
 * don't collide. Falls back to a generic name if the title sanitizes away
 * to nothing (e.g. a title made only of stripped characters). */
export function sanitizeFilename(title: string): string {
  const dateSuffix = new Date().toISOString().slice(0, 10);
  const cleaned = title
    .replace(INVALID_FILENAME_CHARS, " ")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 80);
  return cleaned === "" ? `chat-export-${dateSuffix}` : `${cleaned}-${dateSuffix}`;
}

function toolCallStatusLabel(block: Extract<MessageBlock, { type: "toolCall" }>): string {
  switch (block.status) {
    case "done":
      return "успешно";
    case "error":
      return `ошибка — ${block.errorMessage ?? "неизвестная ошибка"}`;
    case "running":
      return "не завершено (выполнялось на момент экспорта)";
    case "pendingApproval":
      return "не завершено (ожидало подтверждения на момент экспорта)";
  }
}

function formatToolCallBlock(block: Extract<MessageBlock, { type: "toolCall" }>): string {
  let prettyArgs = block.argumentsJson;
  try {
    prettyArgs = JSON.stringify(JSON.parse(block.argumentsJson), null, 2);
  } catch {
    // leave as raw string if it isn't valid JSON
  }
  return [
    `**Вызов инструмента: ${block.name}**`,
    "```json",
    prettyArgs,
    "```",
    `_Статус: ${toolCallStatusLabel(block)}_`,
  ].join("\n");
}

/** Reasoning blocks are deliberately omitted — a model's "thinking" trace
 * is scratch work, not part of the answer the export is meant to capture
 * (the raw JSON export via `chatMessagesToJson` still includes it, since
 * that serializes `ChatMessage[]` as-is). */
function formatBlocks(blocks: MessageBlock[]): string {
  return blocks
    .map((block) => (block.type === "text" ? block.content : block.type === "reasoning" ? "" : formatToolCallBlock(block)))
    .filter((part) => part !== "")
    .join("\n\n");
}

function formatMessage(message: ChatMessage): string {
  if (message.role === "user") {
    return `## Вы\n\n${message.content}`;
  }
  const body = formatBlocks(message.blocks);
  const streamingNote = message.streaming ? "\n\n_(ответ ещё формируется)_" : "";
  const cancelledNote = message.cancelled ? "\n\n_(остановлено пользователем)_" : "";
  const failedNote = message.failed ? "\n\n_(завершилось ошибкой)_" : "";
  return `## Ассистент\n\n${body}${streamingNote}${cancelledNote}${failedNote}`;
}

function formatTodos(todos: Task[]): string {
  if (todos.length === 0) return "";
  const items = todos.map((t) => `- [${t.status === "completed" ? "x" : " "}] ${t.title}`).join("\n");
  return `\n\n---\n\n## Задачи\n\n${items}`;
}

/** Renders a full conversation as a readable Markdown document — headers
 * per turn, tool calls summarized (not dumping full results, which can be
 * arbitrarily large), one export-time timestamp since `ChatMessage` carries
 * no per-message timestamp. */
export function chatMessagesToMarkdown(title: string, messages: ChatMessage[], todos: Task[]): string {
  const exportedAt = new Date().toLocaleString("ru-RU");
  const header = `# ${title}\n\n_Экспортировано: ${exportedAt}_`;
  const body = messages.map(formatMessage).join("\n\n---\n\n");
  return `${header}\n\n---\n\n${body}${formatTodos(todos)}\n`;
}

export type ChatExportJson = {
  exportedAt: string;
  chatId: string;
  title: string;
  messages: ChatMessage[];
  todos: Task[];
};

/** Raw, machine-readable export — `ChatMessage[]` is already plain,
 * JSON-serializable data (see `chatBlocks.ts`'s doc comment), so this is
 * just wrapping it with enough metadata to be self-describing if reopened
 * later. */
export function chatMessagesToJson(chatId: string, title: string, messages: ChatMessage[], todos: Task[]): string {
  const payload: ChatExportJson = {
    exportedAt: new Date().toISOString(),
    chatId,
    title,
    messages,
    todos,
  };
  return JSON.stringify(payload, null, 2);
}

/** Writes already-serialized chat content to an absolute path chosen via
 * the native save dialog — see `commands::export::write_export_file` on the
 * Rust side, a plain unscoped write (unlike `docs_fs::write_project_file`,
 * this path is never under a project's docs_root). */
export function writeExportFile(path: string, content: string): Promise<void> {
  return invoke("write_export_file", { path, content });
}
