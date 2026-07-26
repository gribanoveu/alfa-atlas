import { invoke } from "@tauri-apps/api/core";

export type GitFileStatus = {
  path: string;
  status: string;
};

export type GitStatusSnapshot = {
  staged: GitFileStatus[];
  unstaged: GitFileStatus[];
  branch: string | null;
};

export type GitCommitSummary = {
  hash: string;
  message: string;
  author: string;
  time: number;
};

export type PullMode = "merge" | "rebase";

export type GitDiffScope = "staged" | "unstaged";

export type GitFileDiff = {
  original: string;
  modified: string;
  originalLabel: string;
  modifiedLabel: string;
  isBinary: boolean;
};

export type GitBranchInfo = {
  name: string;
  isCurrent: boolean;
};

export function hasTrackedGitChanges(status: GitStatusSnapshot): boolean {
  return (
    status.staged.length > 0 ||
    status.unstaged.some((file) => file.status !== "?")
  );
}

export type GitSyncStatus = {
  ahead: number;
  behind: number;
};

export function gitStatus(repoRoot: string): Promise<GitStatusSnapshot> {
  return invoke<GitStatusSnapshot>("git_status", { repoRoot });
}

export function gitStage(repoRoot: string, paths: string[]): Promise<void> {
  return invoke<void>("git_stage", { repoRoot, paths });
}

export function gitUnstage(repoRoot: string, paths: string[]): Promise<void> {
  return invoke<void>("git_unstage", { repoRoot, paths });
}

export function gitCommit(
  repoRoot: string,
  message: string,
): Promise<string> {
  return invoke<string>("git_commit", { repoRoot, message });
}

export function gitLog(
  repoRoot: string,
  limit = 20,
): Promise<GitCommitSummary[]> {
  return invoke<GitCommitSummary[]>("git_log", { repoRoot, limit });
}

export function gitPull(repoRoot: string, mode: PullMode): Promise<void> {
  return invoke<void>("git_pull", { repoRoot, mode });
}

export function gitResetToRemote(repoRoot: string): Promise<void> {
  return invoke<void>("git_reset_to_remote", { repoRoot });
}

export function gitSyncStatus(repoRoot: string): Promise<GitSyncStatus> {
  return invoke<GitSyncStatus>("git_sync_status", { repoRoot });
}

export function gitPush(repoRoot: string): Promise<void> {
  return invoke<void>("git_push", { repoRoot });
}

export function gitFileDiff(
  repoRoot: string,
  path: string,
  scope: GitDiffScope,
): Promise<GitFileDiff> {
  return invoke<GitFileDiff>("git_file_diff", { repoRoot, path, scope });
}

export function gitDiscardFileChanges(
  repoRoot: string,
  path: string,
): Promise<void> {
  return invoke<void>("git_discard_file_changes", { repoRoot, path });
}

export function gitListBranches(repoRoot: string): Promise<GitBranchInfo[]> {
  return invoke<GitBranchInfo[]>("git_list_branches", { repoRoot });
}

export function gitCreateBranch(
  repoRoot: string,
  name: string,
  discardChanges = false,
): Promise<void> {
  return invoke<void>("git_create_branch", { repoRoot, name, discardChanges });
}

export function gitCheckoutBranch(
  repoRoot: string,
  name: string,
  discardChanges = false,
): Promise<void> {
  return invoke<void>("git_checkout_branch", { repoRoot, name, discardChanges });
}
