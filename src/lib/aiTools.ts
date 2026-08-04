import { invoke } from "@tauri-apps/api/core";

// Mirrors `domain::ai_access::AiAccessMode` in
// `src-tauri/src/domain/ai_access.rs` (`#[serde(rename_all = "camelCase")]`
// on the enum variants).
export type AiAccessMode = "docsOnly" | "fullRepo";

export type ToolFileEntry = {
  path: string;
  isDir: boolean;
};

// Mirrors the Rust `ToolCall`/`ToolResult` enums in
// `src-tauri/src/domain/ai_tools.rs` (adjacently tagged:
// `#[serde(tag = "tool", content = "args" | "result")]`).
export type ToolCall =
  | { tool: "readFile"; args: { path: string } }
  | { tool: "listFiles"; args: { path: string | null } };

export type ToolResult =
  | { tool: "file"; result: string }
  | { tool: "fileList"; result: ToolFileEntry[] };

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
