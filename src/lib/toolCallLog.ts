import { invoke } from "@tauri-apps/api/core";

// Mirrors `domain::tool_call_log::ToolCallLogRow` — `argsJson`/`resultJson`
// are already redacted server-side (see `infra::tool_call_log::redact_args`/
// `redact_result`) before this ever reaches the frontend.
export type ToolCallLogRow = {
  id: number;
  /** Unix milliseconds. */
  tsMs: number;
  repoRoot: string;
  /** `"chat"` (the LLM tool-calling loop) or `"standalone"` (the
   * `ai_execute_tool` IPC command). */
  source: string;
  round: number | null;
  providerId: string | null;
  model: string | null;
  tool: string;
  argsJson: unknown;
  /** `"ok"` or `"error"`. */
  status: string;
  errorMessage: string | null;
  resultJson: unknown | null;
  durationMs: number;
};

export type ToolCallLogPage = {
  rows: ToolCallLogRow[];
  total: number;
};

// Mirrors `domain::tool_call_log::ToolCallLogFilter` — every field optional,
// absent = no constraint on that dimension.
export type ToolCallLogFilter = {
  repoRoot?: string;
  tool?: string;
  status?: string;
  search?: string;
  sinceMs?: number;
  limit?: number;
  offset?: number;
};

export function queryToolCallLog(filter: ToolCallLogFilter): Promise<ToolCallLogPage> {
  return invoke<ToolCallLogPage>("tool_call_log_query", { filter });
}

/** Deletes every row (`olderThanDays` omitted) or only rows older than that
 * many days. Returns the number of rows deleted. */
export function clearToolCallLog(olderThanDays?: number): Promise<number> {
  return invoke<number>("tool_call_log_clear", { olderThanDays: olderThanDays ?? null });
}
