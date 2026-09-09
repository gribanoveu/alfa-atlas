import { AlertOkModal } from "./AlertOkModal";
import { DeleteBranchConfirmModal } from "./DeleteBranchConfirmModal";
import { DiscardStashConfirmModal } from "./DiscardStashConfirmModal";
import { DropUnpushedConfirmModal } from "./DropUnpushedConfirmModal";
import { GitCommitFileDiffModal } from "./GitCommitFileDiffModal";
import { GitCommitPreviewModal } from "./GitCommitPreviewModal";
import { GitConflictModal } from "./GitConflictModal";
import { GitFileDiffModal } from "./GitFileDiffModal";
import { GitStashPreviewModal } from "./GitStashPreviewModal";
import { MoveUnpushedModal } from "./MoveUnpushedModal";
import { PullUpdateModal } from "./PullUpdateModal";
import { PushConfirmModal } from "./PushConfirmModal";
import { ResetRemoteConfirmModal } from "./ResetRemoteConfirmModal";
import { ConfirmModal } from "../Modals/ConfirmModal";
import { friendlyGitError } from "../../lib/gitErrors";
import type { useBranches } from "../../hooks/useBranches";
import type { useGitPanel } from "../../hooks/useGitPanel";
import type { useGitStash } from "../../hooks/useGitStash";
import type { useGitWorkflow } from "../../hooks/useGitWorkflow";

type GitModalsProps = {
  workflow: ReturnType<typeof useGitWorkflow>;
  git: ReturnType<typeof useGitPanel>;
  branches: ReturnType<typeof useBranches>;
  stash: ReturnType<typeof useGitStash>;
  branchName: string | null;
  editorFontSizePx: number;
};

/** Every dialog `useGitWorkflow` owns, in one place.
 *
 * Extracted from `App` verbatim: fifteen `{state ? <Modal .../> : null}` pairs
 * were ~170 lines of its render, and the fields that drive them are 48 of the
 * 75 `useGitWorkflow` returns — none of which App itself has any other use
 * for. Passing the hook's result whole rather than 48 props is what makes
 * that worth doing; App keeps destructuring only what its own JSX reads. */
export function GitModals({
  workflow,
  git,
  branches,
  stash,
  branchName,
  editorFontSizePx,
}: GitModalsProps) {
  const {
    abortMergeConfirm,
    commitFileDiffTarget,
    commitPreviewTarget,
    conflictTarget,
    currentBranchBehind,
    deleteBranchTarget,
    dropAllUnpushedOpen,
    dropUnpushedTarget,
    gitAlert,
    gitDiffTarget,
    handleDropAllUnpushedConfirm,
    handleDropUnpushedConfirm,
    handleGitDiscard,
    handleGitSaveContent,
    handleMoveUnpushedConfirm,
    loadCommitFileDiff,
    loadCommitFiles,
    loadStashFiles,
    moveUnpushedCommits,
    moveUnpushedOpen,
    onAbortMergeConfirm,
    onConfirmDiscardShelfEntry,
    onDeleteBranchConfirm,
    onPullConfirm,
    onPushConfirm,
    onResetToRemoteConfirm,
    onResolveConflict,
    openCommitFileDiff,
    openCommitPreview,
    openMoveUnpushedModal,
    pullCommits,
    pullCommitsLoading,
    pullModalOpen,
    pushCommits,
    pushCommitsLoading,
    pushConfirmOpen,
    requestDropUnpushed,
    resetRemoteConfirmOpen,
    setAbortMergeConfirm,
    setCommitFileDiffTarget,
    setCommitPreviewTarget,
    setConflictTarget,
    setDeleteBranchTarget,
    setDropAllUnpushedOpen,
    setDropUnpushedTarget,
    setGitAlert,
    setGitDiffTarget,
    setMoveUnpushedOpen,
    setPullModalOpen,
    setPushConfirmOpen,
    setResetRemoteConfirmOpen,
    setStashDiscardTarget,
    setStashPreviewTarget,
    stashDiscardTarget,
    stashPreviewTarget,
    unpushedBusy,
    unpushedHashSet,
  } = workflow;

  return (
    <>
  {pullModalOpen ? (
    <PullUpdateModal
      behind={currentBranchBehind}
      commits={pullCommits}
      commitsLoading={pullCommitsLoading}
      busy={git.busy}
      onCancel={() => setPullModalOpen(false)}
      onConfirm={(mode) => void onPullConfirm(mode)}
      onRequestResetToRemote={() => setResetRemoteConfirmOpen(true)}
      onOpenCommit={(hash) => openCommitPreview(hash, pullCommits)}
    />
  ) : null}

  {resetRemoteConfirmOpen ? (
    <ResetRemoteConfirmModal
      busy={git.busy}
      onCancel={() => setResetRemoteConfirmOpen(false)}
      onConfirm={() => void onResetToRemoteConfirm()}
    />
  ) : null}

  {deleteBranchTarget ? (
    <DeleteBranchConfirmModal
      branch={deleteBranchTarget}
      busy={branches.busy}
      onCancel={() => setDeleteBranchTarget(null)}
      onConfirm={() => void onDeleteBranchConfirm()}
    />
  ) : null}

  {pushConfirmOpen ? (
    <PushConfirmModal
      branchName={branchName}
      hasUpstream={git.status.hasUpstream}
      ahead={git.status.ahead}
      commits={pushCommits}
      commitsLoading={pushCommitsLoading}
      unpushedHashes={unpushedHashSet}
      busy={git.busy || unpushedBusy}
      onCancel={() => setPushConfirmOpen(false)}
      onConfirm={() => void onPushConfirm()}
      onDropCommit={(hash) => requestDropUnpushed(hash, pushCommits)}
      onMoveToBranch={() => openMoveUnpushedModal(pushCommits)}
      onDropAllUnpushed={() => setDropAllUnpushedOpen(true)}
      onOpenCommit={(hash) => openCommitPreview(hash, pushCommits)}
    />
  ) : null}

  {dropUnpushedTarget ? (
    <DropUnpushedConfirmModal
      commit={dropUnpushedTarget.commit}
      newerCount={dropUnpushedTarget.newerCount}
      unpushedCount={git.unpushedCommits.length}
      busy={unpushedBusy}
      onCancel={() => setDropUnpushedTarget(null)}
      onConfirm={(mode) => void handleDropUnpushedConfirm(mode)}
    />
  ) : null}

  {dropAllUnpushedOpen ? (
    <DropUnpushedConfirmModal
      commit={null}
      newerCount={0}
      unpushedCount={git.unpushedCommits.length || git.status.ahead}
      busy={unpushedBusy}
      onCancel={() => setDropAllUnpushedOpen(false)}
      onConfirm={(mode) => void handleDropAllUnpushedConfirm(mode)}
    />
  ) : null}

  {moveUnpushedOpen ? (
    <MoveUnpushedModal
      currentBranch={branchName}
      branches={branches.branches}
      commits={moveUnpushedCommits}
      busy={unpushedBusy}
      onCancel={() => setMoveUnpushedOpen(false)}
      onConfirm={(target) => void handleMoveUnpushedConfirm(target)}
    />
  ) : null}

  {gitDiffTarget ? (
    <GitFileDiffModal
      target={gitDiffTarget}
      busy={git.busy}
      editorFontSizePx={editorFontSizePx}
      onClose={() => setGitDiffTarget(null)}
      onLoadDiff={git.loadFileDiff}
      onDiscard={handleGitDiscard}
      onSaveContent={handleGitSaveContent}
    />
  ) : null}

  {conflictTarget ? (
    <GitConflictModal
      path={conflictTarget}
      busy={git.busy}
      editorFontSizePx={editorFontSizePx}
      onClose={() => setConflictTarget(null)}
      onLoadContent={git.loadConflictFile}
      onResolve={onResolveConflict}
    />
  ) : null}

  {commitPreviewTarget ? (
    <GitCommitPreviewModal
      commit={commitPreviewTarget}
      onClose={() => setCommitPreviewTarget(null)}
      onLoadFiles={loadCommitFiles}
      onOpenFile={openCommitFileDiff}
    />
  ) : null}

  {commitFileDiffTarget ? (
    <GitCommitFileDiffModal
      commitHash={commitFileDiffTarget.commitHash}
      file={commitFileDiffTarget.file}
      editorFontSizePx={editorFontSizePx}
      onClose={() => setCommitFileDiffTarget(null)}
      onLoadDiff={loadCommitFileDiff}
    />
  ) : null}

  {stashPreviewTarget ? (
    <GitStashPreviewModal
      entry={stashPreviewTarget}
      onClose={() => setStashPreviewTarget(null)}
      onLoadFiles={loadStashFiles}
      onOpenFile={(file) => {
        const commitHash = stashPreviewTarget.id;
        setStashPreviewTarget(null);
        openCommitFileDiff(commitHash, file);
      }}
    />
  ) : null}

  {stashDiscardTarget ? (
    <DiscardStashConfirmModal
      branchName={stashDiscardTarget.branch}
      busy={stash.busy}
      onCancel={() => setStashDiscardTarget(null)}
      onConfirm={() => void onConfirmDiscardShelfEntry()}
    />
  ) : null}

  {abortMergeConfirm ? (
    <ConfirmModal
      title={
        abortMergeConfirm.isStashAbort
          ? "Отменить восстановление?"
          : "Отменить слияние?"
      }
      message={
        abortMergeConfirm.isStashAbort
          ? "Рабочая копия вернётся к состоянию до восстановления — сами изменения останутся в разделе «Отложенные изменения»."
          : "Файлы вернутся к состоянию до обновления, изменения с сервера будут отброшены."
      }
      confirmLabel="Отменить"
      cancelLabel="Не отменять"
      danger
      onCancel={() => setAbortMergeConfirm(null)}
      onConfirm={() => void onAbortMergeConfirm()}
    />
  ) : null}

  {gitAlert ? (
    <AlertOkModal
      title={gitAlert.title}
      message={friendlyGitError(gitAlert.message)}
      variant={gitAlert.variant}
      onClose={() => setGitAlert(null)}
    />
  ) : null}
    </>
  );
}
