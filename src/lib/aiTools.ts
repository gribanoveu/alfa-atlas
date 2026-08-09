import { invoke } from "@tauri-apps/api/core";

// Mirrors `domain::ai_access::AiAccessMode` in
// `src-tauri/src/domain/ai_access.rs` (`#[serde(rename_all = "camelCase")]`
// on the enum variants).
export type AiAccessMode = "docsOnly" | "fullRepo";

export type ToolFileEntry = {
  path: string;
  isDir: boolean;
};

// Mirrors `domain::ai_tools::MatchSource`.
export type MatchSource = "semantic" | "lexical" | "symbol";

// Mirrors `domain::ai_tools::ToolMatch`. `score` is only comparable within
// the same `source`: `"semantic"` is `1 - cosineDistance` (higher is
// better), `"lexical"` is a raw substring-occurrence count, `"symbol"` is
// always `1`.
export type ToolMatch = {
  path: string;
  snippet: string;
  score: number;
  startByte: number;
  endByte: number;
  qualifiedName: string | null;
  source: MatchSource;
};

// Mirrors the Rust `ToolCall`/`ToolResult` enums in
// `src-tauri/src/domain/ai_tools.rs` (adjacently tagged:
// `#[serde(tag = "tool", content = "args" | "result")]`).
// One `{old, new}` search-and-replace pair within an `editFile` call —
// `old` must match the target file's current content exactly once, or the
// whole call is rejected (see `domain::ai_tools::FileEdit`).
export type FileEdit = { old: string; new: string };

export type ToolCall =
  | { tool: "readFile"; args: { path: string; startLine: number | null; endLine: number | null } }
  | { tool: "listFiles"; args: { path: string | null; depth: number | null; pattern: string | null } }
  | { tool: "semanticSearch"; args: { query: string; topK: number | null } }
  | { tool: "writeFile"; args: { path: string; content: string } }
  | { tool: "editFile"; args: { path: string; edits: FileEdit[] } }
  | { tool: "createDirectory"; args: { path: string } }
  | { tool: "requestFullRepoAccess"; args: { reason: string } };

// `ToolResult`'s `"file"` case carries range/total-line metadata alongside
// the content — 1-indexed, inclusive, the range actually returned (after
// clamping), not necessarily what was requested. `0`/`0`/`0` on an empty
// file (there is no line 1 to claim).
export type ToolResult =
  | { tool: "file"; result: { content: string; startLine: number; endLine: number; totalLines: number } }
  | { tool: "fileList"; result: ToolFileEntry[] }
  | { tool: "semanticSearchResults"; result: ToolMatch[] }
  | { tool: "fileWritten"; result: { path: string } }
  | { tool: "fileEdited"; result: { path: string } }
  | { tool: "directoryCreated"; result: { path: string } }
  | { tool: "accessModeChanged"; result: { mode: AiAccessMode } };

/**
 * Runs one AI-harness tool call against whichever project is currently
 * open. The caller never passes a docs/repo root or access mode — the
 * backend resolves the current project and its configured `AiAccessMode`/
 * tool allowlist itself (`services::ai_tools::current_scope` in Rust).
 */
export function aiExecuteTool(call: ToolCall): Promise<ToolResult> {
  return invoke<ToolResult>("ai_execute_tool", { call });
}

/** Which part of the filesystem the harness (and `embedding_sync`) may see
 * for the currently open project — "docsOnly" (default) or "fullRepo". */
export function getAiAccessMode(): Promise<AiAccessMode> {
  return invoke<AiAccessMode>("ai_get_access_mode");
}

export function setAiAccessMode(mode: AiAccessMode): Promise<void> {
  return invoke("ai_set_access_mode", { mode });
}

// Mirrors `domain::llm::LlmToolDefinition` in `src-tauri/src/domain/llm.rs`
// (`#[serde(rename_all = "camelCase")]`) — the same definitions actually
// advertised to the model for function-calling.
export type LlmToolDefinition = {
  name: string;
  description: string;
  parameters: unknown;
};

/** The tools currently allowed for the open project's persisted access
 * mode/allowlist — the same source `llm_chat_stream` uses for real
 * function-calling (`services::ai_tools::llm_tool_definitions`). */
export function getToolDefinitions(): Promise<LlmToolDefinition[]> {
  return invoke<LlmToolDefinition[]>("ai_get_tool_definitions");
}
