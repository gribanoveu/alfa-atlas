import { invoke } from "@tauri-apps/api/core";
import type { UpdatedReference } from "./project";
import type { StandardsReport } from "./standards";
import type { Diagnostic } from "./workspaceIndex";

// Mirrors `domain::ai_access::AiAccessMode` in
// `src-tauri/src/domain/ai_access.rs` (`#[serde(rename_all = "camelCase")]`
// on the enum variants).
export type AiAccessMode = "docsOnly" | "fullRepo";

/**
 * Mirrors `domain::ai_tools::CheckKind`. `"problems"` = workspace
 * diagnostics (Problems panel). `"standards"` = the API-documentation
 * corporate standard checker (same engine as the Стандарты panel).
 */
export type CheckKind = "problems" | "standards";

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

// Mirrors `domain::ai_tools::TodoStatus`.
export type TodoStatus = "pending" | "inProgress" | "completed" | "cancelled";

// Mirrors `domain::ai_tools::Task`. Never persisted server-side — owned by
// `useLlmChat`'s `todoListRef` between turns, round-tripped through
// `ChatStreamOutcome`/`PendingApproval` exactly like `history` already is.
export type Task = {
  id: string;
  title: string;
  status: TodoStatus;
  note: string | null;
};

export type ToolCall =
  | { tool: "readFile"; args: { path: string; startLine: number | null; endLine: number | null } }
  | { tool: "listFiles"; args: { path: string | null; depth: number | null; pattern: string | null } }
  | { tool: "semanticSearch"; args: { query: string; topK: number | null } }
  | {
      tool: "grep";
      args: {
        pattern: string;
        path: string | null;
        glob: string | null;
        caseInsensitive: boolean | null;
        maxResults: number | null;
      };
    }
  | { tool: "gitDiff"; args: { path: string; scope: string | null; commit: string | null } }
  | { tool: "gitBlame"; args: { path: string; startLine: number | null; endLine: number | null } }
  | { tool: "check"; args: { kind: CheckKind; path: string | null } }
  | { tool: "writeFile"; args: { path: string; content: string } }
  | { tool: "editFile"; args: { path: string; edits: FileEdit[] } }
  | { tool: "deleteFile"; args: { path: string } }
  | { tool: "createDirectory"; args: { path: string; template: string | null } }
  | { tool: "deleteDirectory"; args: { path: string; recursive: boolean | null } }
  | { tool: "move"; args: { path: string; newPath: string } }
  | { tool: "requestFullRepoAccess"; args: { reason: string } }
  | { tool: "todoWrite"; args: { titles: string[] } }
  | { tool: "todoUpdate"; args: { id: string; status: "completed" | "cancelled"; note: string | null } }
  | {
      tool: "memory";
      args: {
        op: "wake" | "note" | "nap" | "recall" | "zoom" | "forget" | "config";
        scope: "project" | "global";
        text: string | null;
        pattern: string | null;
        block: string | null;
        knob: string | null;
        part: number | null;
        snapshotT: number | null;
      };
    };

// Mirrors Rust's `domain::ai_tools::FileDiffStats` — attached to a settled
// `fileWritten`/`fileEdited`/`fileDeleted` result, computed once
// server-side (`services::text_diff::diff_stats`) and consumed both by the
// chat UI (a `+N -M` badge and colored diff view) and by the model itself
// (the same `ToolResult` JSON is what it reads back). `linesAdded`/
// `linesRemoved` are always the true, untruncated totals even when
// `unifiedDiff` was cut short (`truncated`).
export type FileDiffStats = {
  linesAdded: number;
  linesRemoved: number;
  unifiedDiff: string;
  truncated: boolean;
};

/** Contiguous authorship run from `gitBlame` — mirrors
 * `domain::git::GitBlameHunk`. */
export type GitBlameHunk = {
  startLine: number;
  endLine: number;
  commit: string;
  author: string;
  authoredAt: string;
  summary: string;
};

/** One line hit from `grep` — mirrors `domain::ai_tools::GrepMatch`. */
export type GrepMatch = {
  path: string;
  line: number;
  text: string;
};

// `ToolResult`'s `"file"` case carries range/total-line metadata alongside
// the content — 1-indexed, inclusive, the range actually returned (after
// clamping), not necessarily what was requested. `0`/`0`/`0` on an empty
// file (there is no line 1 to claim).
export type ToolResult =
  | { tool: "file"; result: { content: string; startLine: number; endLine: number; totalLines: number } }
  | { tool: "fileList"; result: ToolFileEntry[] }
  | { tool: "semanticSearchResults"; result: ToolMatch[] }
  | { tool: "grepResults"; result: { matches: GrepMatch[]; truncated: boolean } }
  | { tool: "gitDiff"; result: { path: string; label: string; diff: FileDiffStats; isBinary: boolean } }
  | { tool: "gitBlame"; result: { path: string; hunks: GitBlameHunk[]; truncated: boolean } }
  | { tool: "checkResults"; result: { kind: CheckKind; diagnostics: Diagnostic[]; truncated: boolean } }
  | { tool: "standardsChecked"; result: { report: StandardsReport; truncated: boolean } }
  | { tool: "fileWritten"; result: { path: string; diff: FileDiffStats } }
  | { tool: "fileEdited"; result: { path: string; diff: FileDiffStats } }
  | { tool: "fileDeleted"; result: { path: string; diff: FileDiffStats } }
  | { tool: "directoryCreated"; result: { path: string; template: string | null; createdFiles: string[] } }
  | { tool: "directoryDeleted"; result: { path: string } }
  | { tool: "moved"; result: { from: string; to: string; updatedFiles: UpdatedReference[] } }
  | { tool: "accessModeChanged"; result: { mode: AiAccessMode } }
  | { tool: "todoWritten"; result: Task[] }
  | { tool: "todoUpdated"; result: Task[] }
  | { tool: "memory"; result: { text: string } };

/**
 * Runs one AI-harness tool call against whichever project is currently
 * open. The caller never passes a docs/repo root or access mode — the
 * backend resolves the current project and its configured `AiAccessMode`/
 * tool allowlist itself (`services::ai_tools::current_scope` in Rust).
 */
export function aiExecuteTool(call: ToolCall, todos: Task[] = []): Promise<ToolResult> {
  return invoke<ToolResult>("ai_execute_tool", { call, todos });
}

/** Which part of the filesystem the harness (and `embedding_sync`) may see
 * for the currently open project — "docsOnly" (default) or "fullRepo". */
export function getAiAccessMode(): Promise<AiAccessMode> {
  return invoke<AiAccessMode>("ai_get_access_mode");
}

export function setAiAccessMode(mode: AiAccessMode): Promise<void> {
  return invoke("ai_set_access_mode", { mode });
}

/** Tool names (e.g. `"writeFile"`) the currently open project has persisted
 * as "always allow" via an approval card's "Разрешать всегда" button —
 * loaded once when an assistant chat panel mounts so a choice made in one
 * chat carries into every later chat on this repo. */
export function getAutoApprovedTools(): Promise<string[]> {
  return invoke<string[]>("ai_get_auto_approved_tools");
}

/** Persists (or revokes) one tool's "always allow" status for the currently
 * open project. */
export function setToolAutoApproved(tool: string, autoApproved: boolean): Promise<void> {
  return invoke("ai_set_tool_auto_approved", { tool, autoApproved });
}

/** Tool names the currently open project actually allows right now — the
 * customized `ai_allowed_tools` set if one was ever saved, else the current
 * access mode's default (which is every tool today). */
export function getAllowedTools(): Promise<string[]> {
  return invoke<string[]>("ai_get_allowed_tools");
}

/** Persists (or revokes) one tool's membership in `ai_allowed_tools` for the
 * currently open project. */
export function setToolAllowed(tool: string, allowed: boolean): Promise<void> {
  return invoke("ai_set_tool_allowed", { tool, allowed });
}

/** Combined OptMem wake for project + global stores (injected at chat start). */
export function getMemoryWake(): Promise<string> {
  return invoke<string>("ai_get_memory_wake");
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
