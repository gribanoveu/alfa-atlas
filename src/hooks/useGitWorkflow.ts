import { useCallback, useEffect, useRef, useState } from "react";
import { toMessage } from "../lib/errors";
import type { GitActionLogEntry } from "../lib/gitActionLog";
import {
  deriveSyncPillState,
  gitCommitFileDiff,
  gitSyncStatus,
  gitCommitFiles,
  gitCreateBranchAtOid,
  gitHeadOid,
  gitIncomingCommits,
  gitResetToOid,
  gitRestoreDiscardBackup,
  gitUndoCommit,
  gitUnpushedCommits,
  hasTrackedGitChanges,
  type CheckoutOutcome,
  type GitBranchInfo,
  type GitCommitSummary,
  type GitDiffScope,
  type GitFileDiff,
  type GitFileStatus,
  type GitStashEntry,
  type GitStashRestoreOutcome,
  type PullMode,
} from "../lib/git";
import { toDocsRelativePath } from "../lib/paths";
import type { useBranches } from "./useBranches";
import type { useDocsTree } from "./useDocsTree";
import type { useEditorTabs } from "./useEditorTabs";
import type { useGitActionLog } from "./useGitActionLog";
import type { useGitPanel } from "./useGitPanel";
import type { useGitStash } from "./useGitStash";
import type { useProject } from "./useProject";
import type { useWorkspaceLayout } from "./useWorkspaceLayout";

/** A remote branch name (`origin/feature`) reduced to the local name a
 * checkout would create. */
function localNameFromRemoteBranch(remoteBranchName: string): string {
  const idx = remoteBranchName.indexOf("/");
  return idx < 0 ? remoteBranchName : remoteBranchName.slice(idx + 1);
}

export type GitWorkflowDeps = {
  hasProject: boolean;
  project: ReturnType<typeof useProject>;
  git: ReturnType<typeof useGitPanel>;
  branches: ReturnType<typeof useBranches>;
  stash: ReturnType<typeof useGitStash>;
  actionLog: ReturnType<typeof useGitActionLog>;
  editor: ReturnType<typeof useEditorTabs>;
  tree: ReturnType<typeof useDocsTree>;
  layout: ReturnType<typeof useWorkspaceLayout>;
  /** Shows a transient confirmation. Owned by `App`, which also renders it. */
  showSuccess: (message: string) => void;
};

/** Everything the app does *with* git, as opposed to what `useGitPanel`/
 * `useBranches`/`useGitStash` expose about git's state.
 *
 * Lifted out of `App()`, where these 35 handlers and their dozen modal-state
 * variables were interleaved with unrelated project, panel and assistant
 * concerns. They belong together because they share that state: almost every
 * operation opens or closes one of these dialogs, routes its failure through
 * the same `gitAlert`, and records an undoable entry in the same action log.
 *
 * The handlers keep doing several things at once — call git, move a dialog,
 * raise a toast, record an undo entry — because that is genuinely one
 * user-level operation; what changed is that it is no longer mixed in with
 * everything else the app does. */
function behindCommitsMessage(count: number): string {
  const mod10 = count % 10;
  const mod100 = count % 100;
  let word: string;
  if (mod10 === 1 && mod100 !== 11) {
    word = "новый коммит";
  } else if (mod10 >= 2 && mod10 <= 4 && (mod100 < 10 || mod100 >= 20)) {
    word = "новых коммита";
  } else {
    word = "новых коммитов";
  }
  return `есть ${count} ${word}`;
}

export function useGitWorkflow(deps: GitWorkflowDeps) {
  const {
    hasProject,
    project,
    git,
    branches,
    stash,
    actionLog,
    editor,
    tree,
    layout,
    showSuccess,
  } = deps;

  const [deleteBranchTarget, setDeleteBranchTarget] = useState<GitBranchInfo | null>(null);
  const [pullModalOpen, setPullModalOpen] = useState(false);
  const [resetRemoteConfirmOpen, setResetRemoteConfirmOpen] = useState(false);
  const [pushConfirmOpen, setPushConfirmOpen] = useState(false);
  const [pushCommits, setPushCommits] = useState<GitCommitSummary[]>([]);
  const [pushCommitsLoading, setPushCommitsLoading] = useState(false);
  const [pullCommits, setPullCommits] = useState<GitCommitSummary[]>([]);
  const [pullCommitsLoading, setPullCommitsLoading] = useState(false);
  // Checking out an existing branch no longer blocks on uncommitted changes
  // (see performCheckout/handleCheckoutOutcome — those changes get
  // auto-stashed instead), so this only ever fires for "create branch",
  // where there's no destination tree to stash-restore into.
  const [branchSwitchBlocked, setBranchSwitchBlocked] = useState<{
    branchName: string;
  } | null>(null);
  const [gitAlert, setGitAlert] = useState<{
    message: string;
    title?: string;
    variant?: "error" | "info";
  } | null>(null);
  const [pendingStashConflict, setPendingStashConflict] = useState<{
    id: string;
    branch: string;
  } | null>(null);
  const [gitDiffTarget, setGitDiffTarget] = useState<{
    file: GitFileStatus;
    scope: GitDiffScope;
  } | null>(null);
  const [conflictTarget, setConflictTarget] = useState<string | null>(null);
  const [commitFileDiffTarget, setCommitFileDiffTarget] = useState<{
    commitHash: string;
    file: GitFileStatus;
  } | null>(null);
  const [stashPreviewTarget, setStashPreviewTarget] = useState<GitStashEntry | null>(null);
  const [stashDiscardTarget, setStashDiscardTarget] = useState<GitStashEntry | null>(null);
  useEffect(() => {
    if (layout.activeTool === "branches" && hasProject) {
      void branches.refresh();
    }
  }, [layout.activeTool, hasProject, branches.refresh]);
  useEffect(() => {
    if (layout.activeTool === "git" && hasProject) {
      void git.refresh();
    }
  }, [layout.activeTool, hasProject, git.refresh]);
  const prevConflictCount = useRef(0);
  useEffect(() => {
    const count = git.status.conflicted.length;
    if (count > 0 && prevConflictCount.current === 0) {
      layout.setRightTool("git");
    }
    prevConflictCount.current = count;
  }, [git.status.conflicted.length, layout]);
  // Once a stash-restore conflict (pendingStashConflict) has been fully
  // resolved through the normal conflict UI, the shelved entry is redundant
  // — its content is already merged into the working tree — so drop it
  // quietly. Skipped while a real merge is also in progress so we don't
  // misfire on an unrelated, coincidentally-simultaneous conflict.
  useEffect(() => {
    if (!pendingStashConflict) return;
    if (git.status.conflicted.length > 0) return;
    if (git.status.mergeInProgress) return;
    const id = pendingStashConflict.id;
    setPendingStashConflict(null);
    void stash.discard(id);
  }, [pendingStashConflict, git.status.conflicted.length, git.status.mergeInProgress, stash]);
  const openPullModal = useCallback(() => {
    if (!hasProject) return;
    setPullModalOpen(true);
  }, [hasProject]);
  const openPushModal = useCallback(() => {
    if (!hasProject) return;
    setPushConfirmOpen(true);
  }, [hasProject]);
  const currentBranchBehind =
    branches.branches.find((b) => b.isCurrent)?.behind ?? 0;
  useEffect(() => {
    if (!pushConfirmOpen || !project.repoRoot) {
      setPushCommits([]);
      setPushCommitsLoading(false);
      return;
    }
    let cancelled = false;
    setPushCommitsLoading(true);
    void gitUnpushedCommits(project.repoRoot)
      .then((commits) => {
        if (cancelled) return;
        setPushCommits(commits);
        setPushCommitsLoading(false);
      })
      .catch(() => {
        if (cancelled) return;
        setPushCommits([]);
        setPushCommitsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [pushConfirmOpen, project.repoRoot]);
  useEffect(() => {
    if (!pullModalOpen || !project.repoRoot) {
      setPullCommits([]);
      setPullCommitsLoading(false);
      return;
    }
    let cancelled = false;
    setPullCommitsLoading(true);
    void gitIncomingCommits(project.repoRoot)
      .then((commits) => {
        if (cancelled) return;
        setPullCommits(commits);
        setPullCommitsLoading(false);
      })
      .catch(() => {
        if (cancelled) return;
        setPullCommits([]);
        setPullCommitsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [pullModalOpen, project.repoRoot]);
  const syncPillState = deriveSyncPillState({
    status: git.status,
    currentBranchBehind: branches.branches.find((b) => b.isCurrent)?.behind ?? null,
    hasPendingStashConflict: pendingStashConflict !== null,
  });
  const handleSyncPillClick = useCallback(() => {
    if (syncPillState === "unpushed") {
      setPushConfirmOpen(true);
    } else if (syncPillState === "behind") {
      openPullModal();
    } else {
      layout.setRightTool("git");
    }
  }, [syncPillState, openPullModal, layout]);
  const runPush = useCallback(async () => {
    if (!hasProject || !project.repoRoot) return;
    try {
      const sync = await gitSyncStatus(project.repoRoot);
      if (sync.behind > 0) {
        setGitAlert({
          title: "Сначала обновите проект",
          message: `На сервере ${behindCommitsMessage(sync.behind)}. Выполните «Git → Обновить проект» и повторите отправку.`,
          variant: "info",
        });
        return;
      }
      const err = await git.push();
      setPushConfirmOpen(false);
      if (err) {
        setGitAlert({ message: err });
      } else {
        showSuccess("Изменения отправлены на сервер");
        actionLog.record({
          kind: "push",
          summary: "Изменения отправлены на сервер",
          undoable: false,
          payload: { kind: "push" },
        });
      }
    } catch (e) {
      setPushConfirmOpen(false);
      setGitAlert({
        message: toMessage(e),
      });
    }
  }, [git, hasProject, project.repoRoot, actionLog, showSuccess]);
  const onPullConfirm = useCallback(
    async (mode: PullMode) => {
      const result = await git.pull(mode);
      setPullModalOpen(false);
      if (result.status === "error") {
        setGitAlert({ message: result.message });
      } else if (result.status === "ok") {
        showSuccess("Проект обновлён");
      }
      // "conflict" is surfaced via the Git panel's "Конфликты слияния"
      // section (git.status.conflicted), which auto-opens — no alert here.
    },
    [git, showSuccess],
  );
  const onResetToRemoteConfirm = useCallback(async () => {
    const preResetOid = project.repoRoot
      ? await gitHeadOid(project.repoRoot).catch(() => null)
      : null;
    const err = await git.resetToRemote();
    setResetRemoteConfirmOpen(false);
    setPullModalOpen(false);
    if (err) {
      setGitAlert({ message: err });
    } else if (preResetOid) {
      actionLog.record({
        kind: "resetToRemote",
        summary: "Ветка сброшена к версии на сервере",
        undoable: true,
        payload: { kind: "resetToRemote", preResetOid },
      });
    }
  }, [git, project.repoRoot, actionLog]);
  const onDeleteBranchConfirm = useCallback(async () => {
    if (!deleteBranchTarget) return;
    const { name, tipOid } = deleteBranchTarget;
    const ok = await branches.deleteBranch(name);
    setDeleteBranchTarget(null);
    if (!ok) {
      if (branches.error) setGitAlert({ message: branches.error });
      return;
    }
    if (tipOid) {
      actionLog.record({
        kind: "deleteBranch",
        summary: `Удалена ветка «${name}»`,
        undoable: true,
        payload: { kind: "deleteBranch", name, tipOid },
      });
    }
  }, [branches, deleteBranchTarget, actionLog]);
  const onPushConfirm = useCallback(async () => {
    await runPush();
  }, [runPush]);
  const refreshAfterBranchChange = useCallback(async () => {
    await Promise.all([
      git.refresh(),
      tree.refresh(),
      editor.reloadAllOpenTabs(),
      project.refreshBranch(),
    ]);
  }, [editor.reloadAllOpenTabs, git, project.refreshBranch, tree]);
  // Shared by the auto-restore-on-checkout path and the shelf's manual
  // "Восстановить" button. `conflict`/`blocked`/`skipped` are all
  // guaranteed by the backend to leave the shelf entry intact — never
  // silently dropped — so none of these branches need a fallback recovery
  // path, only messaging pointing at where the entry still lives.
  const handleStashRestoreOutcome = useCallback((restore: GitStashRestoreOutcome) => {
    if (restore.outcome === "applied") {
      showSuccess("Изменения ветки восстановлены");
    } else if (restore.outcome === "conflict") {
      setPendingStashConflict({ id: restore.entry.id, branch: restore.entry.branch });
    } else if (restore.outcome === "blocked") {
      setGitAlert({
        title: "Изменения не восстановлены",
        message: `Не удалось автоматически восстановить отложенные изменения для ветки «${restore.entry.branch}» — ${restore.reason}. Изменения сохранены и доступны в панели Git → «Отложенные изменения».`,
        variant: "info",
      });
    } else if (restore.outcome === "skipped") {
      setGitAlert({
        title: "Изменения не восстановлены",
        message:
          "Для этой ветки в панели Git → «Отложенные изменения» сохранено несколько наборов изменений — выберите нужный вручную.",
        variant: "info",
      });
    }
  }, [showSuccess]);
  // Surfaces what happened as a side effect of a checkout: tracked changes
  // shelved on the branch we left, and/or a shelf entry restored (or not)
  // on the branch we arrived at. See useBranches.checkoutBranch and the
  // backend's checkout_branch/auto_stash_tracked_changes for how this is
  // produced.
  const handleCheckoutOutcome = useCallback(
    (outcome: CheckoutOutcome) => {
      if (outcome.shelved) {
        showSuccess(`Незакоммиченные изменения ветки «${outcome.shelved.branch}» отложены — вернитесь на неё, чтобы восстановить.`);
      }
      if (outcome.restore) handleStashRestoreOutcome(outcome.restore);
    },
    [handleStashRestoreOutcome, showSuccess],
  );
  const performCheckout = useCallback(
    async (name: string, discardChanges: boolean, isRemote: boolean) => {
      const fromBranch = git.status.branch;
      const outcome = isRemote
        ? await branches.checkoutRemoteBranch(name, discardChanges)
        : await branches.checkoutBranch(name, discardChanges);
      if (!outcome) return;
      const toBranch = isRemote ? localNameFromRemoteBranch(name) : name;
      project.setBranchFromGit(toBranch);
      await refreshAfterBranchChange();
      await stash.refresh();
      handleCheckoutOutcome(outcome);
      // Informational only — checkout is already safe by design (auto-stash
      // shelf), so this entry never carries an undo button.
      actionLog.record({
        kind: "checkout",
        summary: fromBranch ? `Переключение: ${fromBranch} → ${toBranch}` : `Переключение на ${toBranch}`,
        undoable: false,
        payload: { kind: "checkout", from: fromBranch ?? "", to: toBranch },
      });
    },
    [branches, project.setBranchFromGit, refreshAfterBranchChange, stash, handleCheckoutOutcome, git.status.branch, actionLog],
  );
  const onRestoreShelfEntry = useCallback(
    async (entry: GitStashEntry) => {
      const restore = await stash.restore(entry.id);
      if (!restore) {
        if (stash.error) setGitAlert({ message: stash.error });
        return;
      }
      await refreshAfterBranchChange();
      handleStashRestoreOutcome(restore);
    },
    [stash, refreshAfterBranchChange, handleStashRestoreOutcome],
  );
  const onDiscardShelfEntry = useCallback((entry: GitStashEntry) => {
    setStashDiscardTarget(entry);
  }, []);
  const onConfirmDiscardShelfEntry = useCallback(async () => {
    if (!stashDiscardTarget) return;
    const { branch } = stashDiscardTarget;
    const ok = await stash.discard(stashDiscardTarget.id);
    setStashDiscardTarget(null);
    if (!ok) {
      if (stash.error) setGitAlert({ message: stash.error });
      return;
    }
    actionLog.record({
      kind: "stashDrop",
      summary: `Отложенные изменения ветки «${branch}» удалены`,
      undoable: false,
      payload: { kind: "stashDrop", branch },
    });
  }, [stash, stashDiscardTarget, actionLog]);
  const onPreviewShelfEntry = useCallback((entry: GitStashEntry) => {
    setStashPreviewTarget(entry);
  }, []);
  const loadStashFiles = useCallback(
    async (stashId: string): Promise<GitFileStatus[] | null> => {
      if (!project.repoRoot) return null;
      try {
        return await gitCommitFiles(project.repoRoot, stashId);
      } catch {
        return null;
      }
    },
    [project.repoRoot],
  );
  // Dispatches an action-log entry's "Отменить" button to the matching
  // backend undo primitive. Never called for `entry.undoable === false`
  // entries (push/stashDrop, or a discard with nothing backed up) — the UI
  // doesn't render an undo button for those in the first place.
  const handleUndoAction = useCallback(
    async (entry: GitActionLogEntry) => {
      if (!project.repoRoot || !entry.undoable) return;
      const repoRoot = project.repoRoot;
      try {
        switch (entry.payload.kind) {
          case "stage":
            await git.unstage(entry.payload.paths);
            break;
          case "unstage":
            await git.stage(entry.payload.paths);
            break;
          case "commit":
          case "mergeCommit":
            await gitUndoCommit(repoRoot, entry.payload.oid);
            break;
          case "deleteBranch":
            await gitCreateBranchAtOid(repoRoot, entry.payload.name, entry.payload.tipOid);
            break;
          case "resetToRemote":
            await gitResetToOid(repoRoot, entry.payload.preResetOid);
            break;
          case "discardFileChanges":
            if (entry.payload.backupStashId) {
              await gitRestoreDiscardBackup(repoRoot, entry.payload.backupStashId);
            }
            break;
          default:
            return;
        }
        actionLog.markUndone(entry.id);
        await Promise.all([git.refresh(), branches.refresh(), stash.refresh()]);
      } catch (e) {
        setGitAlert({ message: toMessage(e) });
      }
    },
    [project.repoRoot, actionLog, git, branches, stash],
  );
  const performCreateBranch = useCallback(
    async (name: string, discardChanges: boolean) => {
      const ok = await branches.createBranch(name, discardChanges);
      if (!ok) return;
      project.setBranchFromGit(name);
      await refreshAfterBranchChange();
    },
    [branches, project.setBranchFromGit, refreshAfterBranchChange],
  );
  // Checking out an existing branch is never blocked on uncommitted
  // changes anymore — performCheckout auto-stashes them (see
  // handleCheckoutOutcome above) instead of forcing a commit-or-discard
  // choice up front.
  const handleCheckoutBranch = useCallback(
    async (branch: GitBranchInfo) => {
      const saved = await editor.saveAllDirtyTabs();
      if (!saved) {
        setGitAlert({
          message: "Не удалось сохранить открытые файлы перед переключением ветки.",
        });
        return;
      }
      await performCheckout(branch.name, false, branch.isRemote);
    },
    [editor.saveAllDirtyTabs, performCheckout],
  );
  const handleCreateBranch = useCallback(
    async (name: string) => {
      const saved = await editor.saveAllDirtyTabs();
      if (!saved) {
        setGitAlert({
          message: "Не удалось сохранить открытые файлы перед созданием ветки.",
        });
        return;
      }
      if (hasTrackedGitChanges(git.status)) {
        setBranchSwitchBlocked({ branchName: name });
        return;
      }
      await performCreateBranch(name, false);
    },
    [editor.saveAllDirtyTabs, git.status, performCreateBranch],
  );
  const handleDiscardAndSwitchBranch = useCallback(async () => {
    if (!branchSwitchBlocked) return;
    const { branchName } = branchSwitchBlocked;
    setBranchSwitchBlocked(null);
    await performCreateBranch(branchName, true);
  }, [branchSwitchBlocked, performCreateBranch]);
  const openGitFileDiff = useCallback(
    (path: string, scope: GitDiffScope) => {
      const file =
        scope === "staged"
          ? git.status.staged.find((f) => f.path === path)
          : git.status.unstaged.find((f) => f.path === path);
      if (!file) return;
      setGitDiffTarget({ file, scope });
    },
    [git.status.staged, git.status.unstaged],
  );
  const openConflict = useCallback((path: string) => {
    setConflictTarget(path);
  }, []);
  const onResolveConflict = useCallback(
    async (path: string, content: string) => {
      const result = await git.resolveConflict(path, content);
      if (result.mergeFinished) {
        showSuccess(result.commitHash
            ? `Слияние завершено, создан коммит ${result.commitHash}`
            : "Слияние завершено");
      }
      return result;
    },
    [git, showSuccess],
  );
  const onAbortMerge = useCallback(async () => {
    // A stash-restore conflict looks identical to a merge conflict from
    // git's perspective (conflicted index entries, no MERGE_HEAD) — same
    // abort machinery, different messaging so the user knows their shelved
    // changes aren't gone, just still sitting in the shelf unapplied.
    const isStashAbort = pendingStashConflict !== null;
    const confirmed = window.confirm(
      isStashAbort
        ? "Отменить восстановление отложенных изменений? Рабочая копия вернётся к состоянию до восстановления — сами изменения останутся в разделе «Отложенные изменения»."
        : "Отменить слияние? Файлы вернутся к состоянию до обновления, изменения с сервера будут отброшены.",
    );
    if (!confirmed) return;
    // Clear before the conflict count can reach zero so the auto-drop
    // effect doesn't mistake this abort for a resolved conflict and drop
    // the shelf entry the user chose to keep.
    setPendingStashConflict(null);
    const ok = await git.abortMerge();
    if (ok) {
      showSuccess(isStashAbort ? "Восстановление отменено" : "Слияние отменено");
    }
  }, [git, pendingStashConflict, showSuccess]);
  const onFinishMergeRetry = useCallback(async () => {
    const ok = await git.finishMerge();
    if (ok) {
      showSuccess("Слияние завершено");
    }
  }, [git, showSuccess]);
  // Thin wrappers around useGitPanel's stage/unstage/commit that additionally
  // record the action log entry — bound directly into <RightDock git={{...}}>
  // below instead of git.stage/git.unstage/git.commit themselves, since
  // those raw hook methods don't know about the log.
  const handleStage = useCallback(
    (paths: string[]) => {
      if (paths.length === 0) return;
      void git.stage(paths);
      actionLog.record({
        kind: "stage",
        summary: paths.length === 1 ? `В индекс добавлен ${paths[0]}` : `В индекс добавлено файлов: ${paths.length}`,
        undoable: true,
        payload: { kind: "stage", paths },
      });
    },
    [git, actionLog],
  );
  const handleUnstage = useCallback(
    (paths: string[]) => {
      if (paths.length === 0) return;
      void git.unstage(paths);
      actionLog.record({
        kind: "unstage",
        summary: paths.length === 1 ? `Из индекса убран ${paths[0]}` : `Из индекса убрано файлов: ${paths.length}`,
        undoable: true,
        payload: { kind: "unstage", paths },
      });
    },
    [git, actionLog],
  );
  const handleCommit = useCallback(() => {
    void git.commit().then((hash) => {
      if (!hash) return;
      actionLog.record({
        kind: "commit",
        summary: `Создан коммит ${hash}`,
        undoable: true,
        payload: { kind: "commit", oid: hash },
      });
    });
  }, [git, actionLog]);
  const openCommitFileDiff = useCallback(
    (commitHash: string, file: GitFileStatus) => {
      setCommitFileDiffTarget({ commitHash, file });
    },
    [],
  );
  const loadCommitFiles = useCallback(
    async (commitHash: string): Promise<GitFileStatus[] | null> => {
      if (!project.repoRoot) return null;
      try {
        return await gitCommitFiles(project.repoRoot, commitHash);
      } catch {
        return null;
      }
    },
    [project.repoRoot],
  );
  const loadCommitFileDiff = useCallback(
    async (commitHash: string, path: string): Promise<GitFileDiff | null> => {
      if (!project.repoRoot) return null;
      try {
        return await gitCommitFileDiff(project.repoRoot, commitHash, path);
      } catch {
        return null;
      }
    },
    [project.repoRoot],
  );
  const syncEditorAfterGitDiscard = useCallback(
    async (repoRelativePath: string) => {
      if (!project.repoRoot || !project.docsRoot) return;
      const docsRel = toDocsRelativePath(
        repoRelativePath,
        project.repoRoot,
        project.docsRoot,
      );
      const reloaded = await editor.reloadTabFromDisk(docsRel);
      if (!reloaded) {
        const tab = editor.tabs.find((t) => t.path === docsRel);
        if (tab) {
          await editor.closeTab(tab.id);
        } else {
          editor.discardTabsUnder(docsRel);
        }
      }
    },
    [editor, project.docsRoot, project.repoRoot],
  );
  const handleGitDiscard = useCallback(
    async (repoRelativePath: string) => {
      const result = await git.discardFileChanges(repoRelativePath);
      if (!result.ok) return false;
      await syncEditorAfterGitDiscard(repoRelativePath);
      setGitDiffTarget(null);
      actionLog.record({
        kind: "discardFileChanges",
        summary: `Отменены изменения в ${repoRelativePath}`,
        undoable: result.backupId !== null,
        payload: {
          kind: "discardFileChanges",
          path: repoRelativePath,
          backupStashId: result.backupId,
        },
      });
      return true;
    },
    [git, syncEditorAfterGitDiscard, actionLog],
  );
  const handleGitSaveContent = useCallback(
    async (repoRelativePath: string, scope: GitDiffScope, content: string) => {
      const ok = await git.applyDiffContent(repoRelativePath, scope, content);
      if (!ok) return false;
      // "staged" writes straight into the index, leaving the working tree
      // (and any open editor tab, which reflects the working tree) untouched.
      if (scope === "unstaged" && project.repoRoot && project.docsRoot) {
        const docsRel = toDocsRelativePath(
          repoRelativePath,
          project.repoRoot,
          project.docsRoot,
        );
        await editor.reloadTabFromDisk(docsRel);
      }
      return true;
    },
    [git, editor, project.docsRoot, project.repoRoot],
  );

  return {
    branchSwitchBlocked,
    commitFileDiffTarget,
    conflictTarget,
    deleteBranchTarget,
    gitAlert,
    gitDiffTarget,
    handleCheckoutBranch,
    handleCommit,
    handleCreateBranch,
    handleDiscardAndSwitchBranch,
    handleGitDiscard,
    handleGitSaveContent,
    handleStage,
    handleSyncPillClick,
    handleUndoAction,
    handleUnstage,
    loadCommitFileDiff,
    loadCommitFiles,
    loadStashFiles,
    onAbortMerge,
    onConfirmDiscardShelfEntry,
    onDeleteBranchConfirm,
    onDiscardShelfEntry,
    onFinishMergeRetry,
    onPreviewShelfEntry,
    onPullConfirm,
    onPushConfirm,
    onResetToRemoteConfirm,
    onResolveConflict,
    onRestoreShelfEntry,
    openCommitFileDiff,
    openConflict,
    openGitFileDiff,
    openPullModal,
    openPushModal,
    pendingStashConflict,
    pullCommits,
    pullCommitsLoading,
    pullModalOpen,
    pushCommits,
    pushCommitsLoading,
    pushConfirmOpen,
    currentBranchBehind,
    resetRemoteConfirmOpen,
    runPush,
    setBranchSwitchBlocked,
    setCommitFileDiffTarget,
    setConflictTarget,
    setDeleteBranchTarget,
    setGitAlert,
    setGitDiffTarget,
    setPullModalOpen,
    setPushConfirmOpen,
    setResetRemoteConfirmOpen,
    setStashDiscardTarget,
    setStashPreviewTarget,
    stashDiscardTarget,
    stashPreviewTarget,
    syncPillState,
  };
}
