import { invoke } from "@tauri-apps/api/core";

export type MemoryLogRow = {
  id: number;
  scope: "project" | "global" | string;
  date: string;
  text: string;
  storePath: string;
};

export type MemoryLogPage = {
  rows: MemoryLogRow[];
  total: number;
  projectStorePath: string | null;
  globalStorePath: string;
};

export type MemoryLogFilter = {
  scope?: string;
  search?: string;
  repoRoot?: string;
  limit?: number;
  offset?: number;
};

export function queryMemoryLog(filter: MemoryLogFilter): Promise<MemoryLogPage> {
  return invoke<MemoryLogPage>("memory_log_query", { filter });
}
