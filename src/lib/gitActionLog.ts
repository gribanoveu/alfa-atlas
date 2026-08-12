import { invoke } from "@tauri-apps/api/core";

export type GitActionKind =
  | "stage"
  | "unstage"
  | "commit"
  | "mergeCommit"
  | "deleteBranch"
  | "resetToRemote"
  | "discardFileChanges"
  | "push"
  | "stashDrop"
  | "checkout";

export type GitActionPayload =
  | { kind: "stage" | "unstage"; paths: string[] }
  | { kind: "commit" | "mergeCommit"; oid: string }
  | { kind: "deleteBranch"; name: string; tipOid: string }
  | { kind: "resetToRemote"; preResetOid: string }
  | { kind: "discardFileChanges"; path: string; backupStashId: string | null }
  | { kind: "push" }
  | { kind: "stashDrop"; branch: string }
  | { kind: "checkout"; from: string; to: string };

export type GitActionLogEntry = {
  id: string;
  kind: GitActionKind;
  summary: string;
  /** Unix milliseconds. */
  createdAt: number;
  undoable: boolean;
  undone: boolean;
  payload: GitActionPayload;
};

export function gitActionLogList(repoRoot: string): Promise<GitActionLogEntry[]> {
  return invoke<GitActionLogEntry[]>("git_action_log_list", { repoRoot });
}

export function gitActionLogAppend(
  repoRoot: string,
  entry: GitActionLogEntry,
): Promise<void> {
  return invoke<void>("git_action_log_append", { repoRoot, entry });
}

export function gitActionLogMarkUndone(id: string): Promise<void> {
  return invoke<void>("git_action_log_mark_undone", { id });
}
