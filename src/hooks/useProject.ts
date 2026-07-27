import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import {
  clearProject,
  getGitBranch,
  getProject,
  getSavedRepoRoot,
  openCachedProject,
  openProject,
  probeOpenPath,
  type ProbeResult,
} from "../lib/project";

export type PendingOpen = ProbeResult;

export function useProject() {
  const [repoRoot, setRepoRoot] = useState<string | null>(null);
  const [docsRoot, setDocsRoot] = useState<string | null>(null);
  const [branchName, setBranchName] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pendingOpen, setPendingOpen] = useState<PendingOpen | null>(null);

  const applyOpened = useCallback(async (root: string, docs: string) => {
    setRepoRoot(root);
    setDocsRoot(docs);
    setError(null);
    setPendingOpen(null);
    try {
      const branch = await getGitBranch(root);
      setBranchName(branch);
    } catch {
      setBranchName(null);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const project = await getProject();
        if (cancelled) return;
        if (project) {
          await applyOpened(project.root, project.docsRoot);
          return;
        }

        const saved = await getSavedRepoRoot();
        if (cancelled || !saved) return;

        const probe = await probeOpenPath(saved);
        if (cancelled) return;
        if (!probe.needsConfirm && probe.docsRoot) {
          const opened = await openCachedProject(probe.root);
          if (!cancelled) await applyOpened(opened.root, opened.docsRoot);
        } else {
          setPendingOpen(probe);
        }
      } catch (e) {
        if (!cancelled) {
          setRepoRoot(null);
          setDocsRoot(null);
          setError(e instanceof Error ? e.message : String(e));
        }
      } finally {
        if (!cancelled) setReady(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [applyOpened]);

  const beginOpenPath = useCallback(
    async (path: string) => {
      const probe = await probeOpenPath(path);
      if (!probe.needsConfirm && probe.docsRoot) {
        const opened = await openCachedProject(probe.root);
        await applyOpened(opened.root, opened.docsRoot);
        return opened;
      }
      setPendingOpen(probe);
      return null;
    },
    [applyOpened],
  );

  const confirmPendingOpen = useCallback(
    async (docsRootPath: string) => {
      if (!pendingOpen) return null;
      const opened = await openProject(pendingOpen.root, docsRootPath);
      await applyOpened(opened.root, opened.docsRoot);
      return opened;
    },
    [applyOpened, pendingOpen],
  );

  const cancelPendingOpen = useCallback(() => {
    setPendingOpen(null);
  }, []);

  /** Accept a pre-computed ProbeResult (e.g. from a clone) and show the confirm modal. */
  const submitProbe = useCallback((probe: PendingOpen) => {
    setPendingOpen(probe);
  }, []);

  const openFolderDialog = useCallback(async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Открыть папку",
    });
    if (selected === null || Array.isArray(selected)) {
      return null;
    }
    return beginOpenPath(selected);
  }, [beginOpenPath]);

  const closeProject = useCallback(async () => {
    await clearProject();
    setRepoRoot(null);
    setDocsRoot(null);
    setBranchName(null);
    setPendingOpen(null);
  }, []);

  const refreshBranch = useCallback(async () => {
    if (!repoRoot) {
      setBranchName(null);
      return;
    }
    try {
      const branch = await getGitBranch(repoRoot);
      setBranchName(branch);
    } catch {
      setBranchName(null);
    }
  }, [repoRoot]);

  const setBranchFromGit = useCallback((branch: string | null) => {
    setBranchName(branch);
  }, []);

  const projectName = repoRoot
    ? (repoRoot.split(/[/\\]/).filter(Boolean).pop() ?? repoRoot)
    : null;

  return {
    repoRoot,
    docsRoot,
    projectRoot: repoRoot,
    projectName,
    branchName,
    ready,
    error,
    pendingOpen,
    applyOpened,
    openFolderDialog,
    beginOpenPath,
    confirmPendingOpen,
    cancelPendingOpen,
    submitProbe,
    closeProject,
    refreshBranch,
    setBranchFromGit,
  };
}
