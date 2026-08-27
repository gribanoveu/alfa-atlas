import { invoke } from "@tauri-apps/api/core";

export type GitFileStatus = {
  path: string;
  status: string;
};

export type GitStatusSnapshot = {
  staged: GitFileStatus[];
  unstaged: GitFileStatus[];
  /** Files with unresolved merge conflicts (status "U"). Only populated while mergeInProgress is true. */
  conflicted: GitFileStatus[];
  branch: string | null;
  /** Whether HEAD resolves to a commit (false on a brand-new, empty repo). */
  hasCommits: boolean;
  /** Whether the current branch has an upstream remote-tracking branch configured. */
  hasUpstream: boolean;
  /** Commits on HEAD not yet on the upstream (local-only; 0 when hasUpstream is false). */
  ahead: number;
  /** Whether a merge was left unfinished by a conflict (MERGE_HEAD present). */
  mergeInProgress: boolean;
};

export type GitConflictFile = {
  path: string;
  content: string;
};

export function hasUnpushedCommits(status: GitStatusSnapshot): boolean {
  if (!status.hasCommits) return false;
  return !status.hasUpstream || status.ahead > 0;
}

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
  isRemote: boolean;
  /** Commits on the upstream not yet pulled locally; null if no upstream. */
  behind: number | null;
  /** The branch's tip commit oid — used to undo a branch deletion. */
  tipOid: string | null;
};

export function hasTrackedGitChanges(status: GitStatusSnapshot): boolean {
  return (
    status.staged.length > 0 ||
    status.unstaged.some((file) => file.status !== "?")
  );
}

/** Coarse-grained summary of repo sync state for the TopBar pill. Distinct
 * from GitSyncStatus (the live {ahead,behind} fetch-check shape below) —
 * this is a UI-facing enum derived from already-available snapshot data,
 * recomputed on the same cadence git.status/branches already refresh on
 * (no new polling). */
export type SyncPillState =
  | "conflict"
  | "merging"
  | "dirty"
  | "behind"
  | "unpushed"
  | "synced";

export type SyncPillInput = {
  status: GitStatusSnapshot;
  /** `behind` of the *current* branch from GitBranchInfo — a last-fetch snapshot, not live. */
  currentBranchBehind: number | null;
  /** Whether a stash-restore conflict is pending (App.tsx's pendingStashConflict). */
  hasPendingStashConflict: boolean;
};

/** Precedence: conflict > merging > dirty > behind > unpushed > synced.
 * `dirty` outranks `behind`/`unpushed` because it's the immediate local
 * loss-risk; `behind` outranks `unpushed` because pushing while behind
 * needs a pull first anyway — telling the user to pull is the more useful
 * of the two messages. */
export function deriveSyncPillState(input: SyncPillInput): SyncPillState {
  const { status, currentBranchBehind, hasPendingStashConflict } = input;
  if (status.conflicted.length > 0 || hasPendingStashConflict) return "conflict";
  if (status.mergeInProgress) return "merging";
  if (hasTrackedGitChanges(status)) return "dirty";
  if ((currentBranchBehind ?? 0) > 0) return "behind";
  if (hasUnpushedCommits(status)) return "unpushed";
  return "synced";
}

export function syncPillLabel(state: SyncPillState): string {
  switch (state) {
    case "conflict":
      return "Конфликт файлов";
    case "merging":
      return "Слияние в процессе";
    case "dirty":
      return "Есть несохранённые правки";
    case "behind":
      return "Есть новые изменения";
    case "unpushed":
      return "Нужно отправить";
    case "synced":
      return "Все синхронизировано";
  }
}

/** Compact label for the TopBar sync pill — full text stays in `title`. */
export function syncPillShortLabel(state: SyncPillState): string {
  switch (state) {
    case "conflict":
      return "Конфликт";
    case "merging":
      return "Слияние";
    case "dirty":
      return "Изменения";
    case "behind":
      return "Обновить";
    case "unpushed":
      return "Отправить";
    case "synced":
      return "Синхронизировано";
  }
}

export type GitSyncStatus = {
  ahead: number;
  behind: number;
};

/** One shelved (auto-stashed) set of tracked working-tree changes. */
export type GitStashEntry = {
  id: string;
  branch: string;
  createdAt: number;
  filesChanged: number;
};

export type GitStashRestoreOutcome =
  | { outcome: "applied"; entry: GitStashEntry }
  | { outcome: "conflict"; entry: GitStashEntry }
  | { outcome: "blocked"; entry: GitStashEntry; reason: string }
  | { outcome: "skipped"; count: number };

export type CheckoutOutcome = {
  shelved: GitStashEntry | null;
  restore: GitStashRestoreOutcome | null;
};

export type GitProgressEvent =
  | { kind: "started"; op: string }
  | {
      kind: "transfer";
      op: string;
      receivedObjects: number;
      totalObjects: number;
      receivedBytes: number;
      indexedDeltas: number;
      totalDeltas: number;
    }
  | { kind: "push"; op: string; current: number; total: number; bytes: number }
  | { kind: "finished"; op: string };

export const GIT_PROGRESS_EVENT = "git://progress";

export type SshKeySource =
  | { kind: "keyContent"; privateKey: string }
  | { kind: "keyFile"; path: string };

export type SshKeyConfig = {
  name: string;
  host?: string;
  source: SshKeySource;
  passphrase?: string;
};

export type GitCredentials = {
  sshKeys: SshKeyConfig[];
  trustAllSshHostKeys: boolean;
};

export type AppKeyStatus = {
  exists: boolean;
  publicKey: string;
  privateKeyAvailable: boolean;
  isImported: boolean;
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

export function gitUnpushedCommits(
  repoRoot: string,
  limit = 50,
): Promise<GitCommitSummary[]> {
  return invoke<GitCommitSummary[]>("git_unpushed_commits", { repoRoot, limit });
}

export function gitIncomingCommits(
  repoRoot: string,
  limit = 50,
): Promise<GitCommitSummary[]> {
  return invoke<GitCommitSummary[]>("git_incoming_commits", { repoRoot, limit });
}

export function gitPull(repoRoot: string, mode: PullMode): Promise<void> {
  return invoke<void>("git_pull", { repoRoot, mode });
}

export function gitConflictFileContent(
  repoRoot: string,
  path: string,
): Promise<GitConflictFile> {
  return invoke<GitConflictFile>("git_conflict_file_content", { repoRoot, path });
}

export function gitResolveConflict(
  repoRoot: string,
  path: string,
  content: string,
): Promise<void> {
  return invoke<void>("git_resolve_conflict", { repoRoot, path, content });
}

export function gitFinishMerge(repoRoot: string): Promise<string> {
  return invoke<string>("git_finish_merge", { repoRoot });
}

export function gitAbortMerge(repoRoot: string): Promise<void> {
  return invoke<void>("git_abort_merge", { repoRoot });
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

export function gitCommitFiles(
  repoRoot: string,
  commitHash: string,
): Promise<GitFileStatus[]> {
  return invoke<GitFileStatus[]>("git_commit_files", { repoRoot, commitHash });
}

export function gitCommitFileDiff(
  repoRoot: string,
  commitHash: string,
  path: string,
): Promise<GitFileDiff> {
  return invoke<GitFileDiff>("git_commit_file_diff", {
    repoRoot,
    commitHash,
    path,
  });
}

/** Discards `path`'s uncommitted changes, taking a backup first so the
 * discard is undoable. Returns the backup id (opaque — pass to
 * gitRestoreDiscardBackup to undo) or null if there was nothing to
 * discard. */
export function gitDiscardFileChanges(
  repoRoot: string,
  path: string,
): Promise<string | null> {
  return invoke<string | null>("git_discard_file_changes", { repoRoot, path });
}

export function gitRestoreDiscardBackup(
  repoRoot: string,
  backupId: string,
): Promise<void> {
  return invoke<void>("git_restore_discard_backup", { repoRoot, backupId });
}

export function gitUndoCommit(repoRoot: string, commitHash: string): Promise<void> {
  return invoke<void>("git_undo_commit", { repoRoot, commitHash });
}

export function gitCreateBranchAtOid(
  repoRoot: string,
  name: string,
  oid: string,
): Promise<void> {
  return invoke<void>("git_create_branch_at_oid", { repoRoot, name, oid });
}

export function gitResetToOid(repoRoot: string, oid: string): Promise<void> {
  return invoke<void>("git_reset_to_oid", { repoRoot, oid });
}

export function gitHeadOid(repoRoot: string): Promise<string> {
  return invoke<string>("git_head_oid", { repoRoot });
}

export function gitApplyDiffContent(
  repoRoot: string,
  path: string,
  scope: GitDiffScope,
  content: string,
): Promise<void> {
  return invoke<void>("git_apply_diff_content", { repoRoot, path, scope, content });
}

export function gitListBranches(repoRoot: string): Promise<GitBranchInfo[]> {
  return invoke<GitBranchInfo[]>("git_list_branches", { repoRoot });
}

export function gitFetchBranches(repoRoot: string): Promise<void> {
  return invoke<void>("git_fetch_branches", { repoRoot });
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
): Promise<CheckoutOutcome> {
  return invoke<CheckoutOutcome>("git_checkout_branch", { repoRoot, name, discardChanges });
}

export function gitDeleteBranch(
  repoRoot: string,
  name: string,
): Promise<void> {
  return invoke<void>("git_delete_branch", { repoRoot, name });
}

export function gitCheckoutRemoteBranch(
  repoRoot: string,
  name: string,
  discardChanges = false,
): Promise<CheckoutOutcome> {
  return invoke<CheckoutOutcome>("git_checkout_remote_branch", {
    repoRoot,
    name,
    discardChanges,
  });
}

export function gitStashList(repoRoot: string): Promise<GitStashEntry[]> {
  return invoke<GitStashEntry[]>("git_stash_list", { repoRoot });
}

export function gitStashApply(
  repoRoot: string,
  stashId: string,
): Promise<GitStashRestoreOutcome> {
  return invoke<GitStashRestoreOutcome>("git_stash_apply", { repoRoot, stashId });
}

export function gitStashDrop(repoRoot: string, stashId: string): Promise<void> {
  return invoke<void>("git_stash_drop", { repoRoot, stashId });
}

export function gitGetCredentials(): Promise<GitCredentials> {
  return invoke<GitCredentials>("git_get_credentials");
}

export function gitSaveCredentials(
  credentials: GitCredentials,
): Promise<void> {
  return invoke<void>("git_save_credentials", { credentials });
}

import { type ProbeResult } from "./project";

export type { ProbeResult };

export function gitClone(
  url: string,
  destination: string,
): Promise<ProbeResult> {
  return invoke<ProbeResult>("git_clone", { url, destination });
}

export function gitGetKeyStatus(): Promise<AppKeyStatus> {
  return invoke<AppKeyStatus>("git_get_key_status");
}

export function gitGenerateKey(): Promise<AppKeyStatus> {
  return invoke<AppKeyStatus>("git_generate_key");
}

export function gitImportKey(sourcePath: string): Promise<AppKeyStatus> {
  return invoke<AppKeyStatus>("git_import_key", { sourcePath });
}
