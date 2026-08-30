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
import { toMessage } from "../lib/errors";
import { trackMetric } from "../lib/metrics";
import { METRICS, type ProjectOpenSource } from "../data/metricsCatalog";

export type PendingOpen = ProbeResult;

export function useProject() {
  const [repoRoot, setRepoRoot] = useState<string | null>(null);
  const [docsRoot, setDocsRoot] = useState<string | null>(null);
  const [branchName, setBranchName] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pendingOpen, setPendingOpen] = useState<PendingOpen | null>(null);
  /** How the pending open started, so the confirm path reports the same source. */
  const [pendingSource, setPendingSource] = useState<ProjectOpenSource>("dialog");

  const applyOpened = useCallback(
    async (root: string, docs: string, source: ProjectOpenSource) => {
      setRepoRoot(root);
      setDocsRoot(docs);
      setError(null);
      setPendingOpen(null);
      let isRepo = false;
      try {
        const branch = await getGitBranch(root);
        setBranchName(branch);
        isRepo = branch !== null;
      } catch {
        setBranchName(null);
      }
      // Only whether it is a repository and how it was opened — never the
      // path or the repository's name.
      void trackMetric(METRICS.APP.OPEN_PROJECT, undefined, {
        label: isRepo ? "git" : "plain",
        property: source,
      });
    },
    [],
  );

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const project = await getProject();
        if (cancelled) return;
        if (project) {
          await applyOpened(project.root, project.docsRoot, "restore");
          return;
        }

        const saved = await getSavedRepoRoot();
        if (cancelled || !saved) return;

        const probe = await probeOpenPath(saved);
        if (cancelled) return;
        if (!probe.needsConfirm && probe.docsRoot) {
          const opened = await openCachedProject(probe.root);
          if (!cancelled) await applyOpened(opened.root, opened.docsRoot, "restore");
        } else {
          setPendingOpen(probe);
        }
      } catch (e) {
        if (!cancelled) {
          setRepoRoot(null);
          setDocsRoot(null);
          setError(toMessage(e));
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
    async (path: string, source: ProjectOpenSource = "recent") => {
      const probe = await probeOpenPath(path);
      if (!probe.needsConfirm && probe.docsRoot) {
        const opened = await openCachedProject(probe.root);
        await applyOpened(opened.root, opened.docsRoot, source);
        return opened;
      }
      setPendingSource(source);
      setPendingOpen(probe);
      return null;
    },
    [applyOpened],
  );

  const confirmPendingOpen = useCallback(
    async (docsRootPath: string) => {
      if (!pendingOpen) return null;
      const opened = await openProject(pendingOpen.root, docsRootPath);
      await applyOpened(opened.root, opened.docsRoot, pendingSource);
      return opened;
    },
    [applyOpened, pendingOpen, pendingSource],
  );

  const cancelPendingOpen = useCallback(() => {
    setPendingOpen(null);
  }, []);

  /** Accept a pre-computed ProbeResult (e.g. from a clone) and show the confirm modal. */
  const submitProbe = useCallback(
    (probe: PendingOpen, source: ProjectOpenSource = "clone") => {
      setPendingSource(source);
      setPendingOpen(probe);
    },
    [],
  );

  const openFolderDialog = useCallback(async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Открыть папку",
    });
    if (selected === null || Array.isArray(selected)) {
      return null;
    }
    return beginOpenPath(selected, "dialog");
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
