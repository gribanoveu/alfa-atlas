import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { BottomDock } from "./components/BottomDock/BottomDock";
import { EditorPane } from "./components/Editor/Editor";
import { AlertOkModal } from "./components/Git/AlertOkModal";
import { GitFileDiffModal } from "./components/Git/GitFileDiffModal";
import { CheckoutBlockedModal } from "./components/Git/CheckoutBlockedModal";
import { PullUpdateModal } from "./components/Git/PullUpdateModal";
import { ResetRemoteConfirmModal } from "./components/Git/ResetRemoteConfirmModal";
import { RightDock } from "./components/RightDock/RightDock";
import { NewFileModal } from "./components/Sidebar/NewFileModal";
import { NewFolderModal } from "./components/Sidebar/NewFolderModal";
import { DeleteConfirmModal } from "./components/Sidebar/DeleteConfirmModal";
import { Sidebar } from "./components/Sidebar/Sidebar";
import { StatusBar } from "./components/StatusBar/StatusBar";
import { TopBar } from "./components/TopBar/TopBar";
import { ConfirmOpenProjectModal } from "./components/Welcome/ConfirmOpenProjectModal";
import { Welcome } from "./components/Welcome/Welcome";
import type { GitBranchInfo, GitDiffScope, GitFileStatus, PullMode } from "./lib/git";
import { gitSyncStatus, hasTrackedGitChanges } from "./lib/git";
import { useBranches } from "./hooks/useBranches";
import { useDocsTree } from "./hooks/useDocsTree";
import { useEditorTabs } from "./hooks/useEditorTabs";
import { useGeneralPrefs } from "./hooks/useGeneralPrefs";
import { useGitPanel } from "./hooks/useGitPanel";
import { usePanelLayout } from "./hooks/usePanelLayout";
import { useProject } from "./hooks/useProject";
import { useAsciiDocParser } from "./hooks/useAsciiDocParser";
import { useEditorViewMode } from "./hooks/useEditorViewMode";
import { useWorkspaceIndex } from "./hooks/useWorkspaceIndex";
import { findAnchors } from "./lib/workspaceIndex";
import { useWorkspaceLayout } from "./hooks/useWorkspaceLayout";
import {
  collectDirPaths,
  useWorkspaceSession,
} from "./hooks/useWorkspaceSession";
import { copyProjectDir, copyProjectFile, createProjectDir, createProjectFile, deleteProjectDir, deleteProjectFile, readProjectFile, renameProjectDir, renameProjectFile } from "./lib/project";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { FileTreeDeleteTarget } from "./components/Sidebar/FileTree";
import { formatLabelFor, isAsciiDocPath, lineEndingLabelFor } from "./lib/supportedFiles";
import { toDocsRelativePath, toRepoRelativePath } from "./lib/paths";
import { RenameModal } from "./components/Sidebar/RenameModal";

function joinParent(parentPath: string, name: string): string {
  if (!parentPath || parentPath === ".") return name;
  return `${parentPath.replace(/[/\\]+$/, "")}/${name}`;
}

/** Append " copy" to a name. For files it goes before the extension
 * (`report.adoc` → `report copy.adoc`) so the copy keeps a supported
 * extension; for directories it goes at the very end. */
function withCopySuffix(name: string, isDir: boolean): string {
  if (!isDir) {
    const dot = name.lastIndexOf(".");
    if (dot > 0) {
      return `${name.slice(0, dot)} copy${name.slice(dot)}`;
    }
  }
  return `${name} copy`;
}

/** Local branch name a remote branch checkout would create/switch to
 * (`origin/feature-x` → `feature-x`), mirroring the Rust-side logic in
 * `checkout_remote_branch`. */
function localNameFromRemoteBranch(remoteBranchName: string): string {
  const idx = remoteBranchName.indexOf("/");
  return idx < 0 ? remoteBranchName : remoteBranchName.slice(idx + 1);
}

function parentOfPath(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  if (parts.length <= 1) return ".";
  return parts.slice(0, -1).join("/");
}

/**
 * Resolve an xref `href` (`path#anchor`, `path`, or `#anchor`) against the
 * docs-relative `sourcePath` of the document that contains the link. Returns
 * a `{ path, anchor }` pair where `path` is docs-relative (suitable for
 * `editor.openFile`) and `anchor` may be `null`.
 *
 * When `href` has no path component (just `#anchor`), the target is the
 * current document — `path` is `sourcePath` unchanged.
 */
function resolveXrefHref(
  href: string,
  sourcePath: string,
): { path: string; anchor: string | null } {
  // Strip any `./`/`../`-style relative segments against the source file's
  // directory. We don't use `URL` because these hrefs are not real URLs
  // (no scheme) and Tauri webview may absolutize them oddly.
  const hashIdx = href.indexOf("#");
  const pathPart = hashIdx >= 0 ? href.slice(0, hashIdx) : href;
  const anchor = hashIdx >= 0 ? href.slice(hashIdx + 1) : null;

  if (!pathPart) {
    return { path: sourcePath, anchor: anchor ?? null };
  }

  const baseDir = sourcePath.includes("/")
    ? sourcePath.slice(0, sourcePath.lastIndexOf("/"))
    : "";
  const combined = baseDir ? `${baseDir}/${pathPart}` : pathPart;
  const normalized = normalizeRelativePath(combined);
  return { path: normalized, anchor: anchor ?? null };
}

/** Collapse `.`/`..` segments in a `/`-joined relative path. */
function normalizeRelativePath(p: string): string {
  const norm = p.replace(/\\/g, "/");
  const stack: string[] = [];
  for (const segment of norm.split("/")) {
    if (segment === "" || segment === ".") continue;
    if (segment === "..") {
      stack.pop();
      continue;
    }
    stack.push(segment);
  }
  return stack.join("/") || ".";
}

function App() {
  // Mount the AsciiDoc parse-request listener unconditionally. The hook
  // registers the `asciidoc:parse-requested` event listener and signals
  // `frontend_ready` to the Rust coordinator so buffered parse requests
  // can be drained.
  useAsciiDocParser();
  const layout = useWorkspaceLayout();
  const viewMode = useEditorViewMode();
  const project = useProject();
  const generalPrefs = useGeneralPrefs();
  const panels = usePanelLayout(project.repoRoot, {
    onCollapseSidebar: () => layout.setSidebarOpen(false),
    onCollapseRight: () => layout.setRightTool(null),
    onCollapseBottom: () => layout.setBottomToolId(null),
  });
  const tree = useDocsTree(project.docsRoot);
  const session = useWorkspaceSession(project.repoRoot, project.docsRoot);

  const onTabsChange = useCallback(
    (openTabs: string[], activeTab: string | null) => {
      session.syncTabs(openTabs, activeTab);
    },
    [session.syncTabs],
  );

  const editor = useEditorTabs(project.docsRoot, {
    onTabsChange,
    prefs: generalPrefs.prefs,
  });
  const git = useGitPanel(project.repoRoot, {
    active: Boolean(project.repoRoot),
    onBranchChange: project.setBranchFromGit,
  });
  const branches = useBranches(project.repoRoot, {
    active: Boolean(project.repoRoot),
  });
  const [folderError, setFolderError] = useState<string | null>(null);
  const [dismissedToastMessage, setDismissedToastMessage] = useState<string | null>(null);
  const [newFileParent, setNewFileParent] = useState<string | null>(null);
  const [newFolderParent, setNewFolderParent] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<FileTreeDeleteTarget | null>(null);
  const [copiedItem, setCopiedItem] = useState<FileTreeDeleteTarget | null>(null);
  const [renameTarget, setRenameTarget] = useState<FileTreeDeleteTarget | null>(null);
  const [pullModalOpen, setPullModalOpen] = useState(false);
  const [resetRemoteConfirmOpen, setResetRemoteConfirmOpen] = useState(false);
  const [branchSwitchBlocked, setBranchSwitchBlocked] = useState<{
    kind: "checkout" | "create";
    branchName: string;
    isRemote?: boolean;
  } | null>(null);
  const [gitAlert, setGitAlert] = useState<{
    message: string;
    title?: string;
    variant?: "error" | "info";
  } | null>(null);
  const [gitDiffTarget, setGitDiffTarget] = useState<{
    file: GitFileStatus;
    scope: GitDiffScope;
  } | null>(null);
  const [revealRequest, setRevealRequest] = useState<{
    id: number;
    line: number;
    column: number;
    severity: "error" | "warning";
  } | null>(null);
  const [insertRequest, setInsertRequest] = useState<{
    id: number;
    tabId: string;
    text: string;
  } | null>(null);
  const revealCounter = useRef(0);
  const insertCounter = useRef(0);
  const skipNextPanelSync = useRef(false);
  const prevDirtyCount = useRef(0);

  useEffect(() => {
    if (!session.ready || !session.loadedState || !project.docsRoot) return;
    void editor.restoreTabs(
      session.loadedState.openTabs,
      session.loadedState.activeTab,
    );
  }, [
    session.ready,
    session.loadedState,
    project.docsRoot,
    editor.restoreTabs,
  ]);

  useEffect(() => {
    if (!session.ready || !session.loadedState || !project.docsRoot) return;
    skipNextPanelSync.current = true;
    layout.hydrate(session.loadedState);
  }, [
    session.ready,
    session.loadedState,
    project.docsRoot,
    layout.hydrate,
  ]);

  useEffect(() => {
    if (!session.ready || !project.docsRoot) return;
    if (skipNextPanelSync.current) {
      skipNextPanelSync.current = false;
      return;
    }
    session.syncPanelUi({
      sidebarOpen: layout.sidebarOpen,
      rightTool: layout.activeTool,
      bottomTool: layout.bottomTool,
    });
  }, [
    session.ready,
    session.syncPanelUi,
    project.docsRoot,
    layout.sidebarOpen,
    layout.activeTool,
    layout.bottomTool,
  ]);

  useEffect(() => {
    if (!session.ready || !session.loadedState || !project.docsRoot) return;
    if (tree.nodes.length === 0) return;
    const loaded = session.loadedState.expandedDirs;
    const isDefault =
      loaded.length === 0 || (loaded.length === 1 && loaded[0] === ".");
    if (!isDefault) return;
    if (session.expandedDirs.size > 1) return;
    session.seedShallowExpanded(collectDirPaths(tree.nodes));
  }, [
    session.ready,
    session.loadedState,
    session.expandedDirs.size,
    session.seedShallowExpanded,
    project.docsRoot,
    tree.nodes,
  ]);

  const mainClassName = [
    "main",
    layout.sidebarOpen ? "" : "sidebar-collapsed",
    layout.activeTool ? "" : "right-collapsed",
  ]
    .filter(Boolean)
    .join(" ");

  const hasProject = Boolean(project.docsRoot && project.repoRoot);
  const workspaceIndex = useWorkspaceIndex(project.repoRoot, {
    active: hasProject,
  });

  useEffect(() => {
    if (layout.activeTool === "branches" && hasProject) {
      void branches.refresh();
    }
  }, [layout.activeTool, hasProject, branches.refresh]);

  const cursorLabel = hasProject
    ? `Ln ${editor.cursor.line}, Col ${editor.cursor.column}`
    : "Ln 1, Col 1";

  const openPullModal = useCallback(() => {
    if (!hasProject) return;
    setPullModalOpen(true);
  }, [hasProject]);

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

  const runPush = useCallback(async () => {
    if (!hasProject || !project.repoRoot) return;
    try {
      const sync = await gitSyncStatus(project.repoRoot);
      if (sync.behind > 0) {
        setGitAlert({
          title: "Сначала обновите проект",
          message: `На сервере ${behindCommitsMessage(sync.behind)}. Выполните «Git → Pull» и повторите отправку.`,
          variant: "info",
        });
        return;
      }
      const err = await git.push();
      if (err) setGitAlert({ message: err });
    } catch (e) {
      setGitAlert({
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [git, hasProject, project.repoRoot]);

  const onPullConfirm = useCallback(
    async (mode: PullMode) => {
      const err = await git.pull(mode);
      setPullModalOpen(false);
      if (err) setGitAlert({ message: err });
    },
    [git],
  );

  const onResetToRemoteConfirm = useCallback(async () => {
    const err = await git.resetToRemote();
    setResetRemoteConfirmOpen(false);
    setPullModalOpen(false);
    if (err) setGitAlert({ message: err });
  }, [git]);

  const panelStyle = {
    ["--sidebar-width" as string]: `${panels.layout.sidebarWidth}px`,
    ["--right-width" as string]: `${panels.layout.rightWidth}px`,
    ["--bottom-height" as string]: `${panels.layout.bottomHeight}px`,
    ["--external-height" as string]: `${panels.layout.externalHeight}px`,
    ["--font-ui" as string]: `${generalPrefs.prefs.uiFontSizePx}px`,
    ["--font-sidebar" as string]: `${generalPrefs.prefs.sidebarFontSizePx}px`,
    ["--font-editor" as string]: `${generalPrefs.prefs.editorFontSizePx}px`,
    ["--font-preview" as string]: `${generalPrefs.prefs.previewFontSizePx}px`,
  };

  const openFolder = useCallback(async () => {
    setFolderError(null);
    try {
      await project.openFolderDialog();
    } catch (e) {
      setFolderError(e instanceof Error ? e.message : String(e));
    }
  }, [project]);

  const openRecent = useCallback(
    async (root: string) => {
      setFolderError(null);
      try {
        await project.beginOpenPath(root);
      } catch (e) {
        setFolderError(e instanceof Error ? e.message : String(e));
        throw e;
      }
    },
    [project],
  );

  const closeProject = useCallback(async () => {
    await editor.reset();
    await project.closeProject();
    skipNextPanelSync.current = true;
    layout.reset();
  }, [editor.reset, layout.reset, project.closeProject]);

  const toggleRightPanel = useCallback(() => {
    if (layout.activeTool) {
      layout.setRightTool(null);
    } else {
      layout.setRightTool("assistant");
    }
  }, [layout]);

  const toggleBottomPanel = useCallback(() => {
    if (layout.bottomTool) {
      layout.setBottomToolId(null);
    } else {
      layout.setBottomToolId("suggestions");
    }
  }, [layout]);

  const toggleGitPanel = useCallback(() => {
    layout.toggleRightTool("git");
  }, [layout]);

  const refreshAfterBranchChange = useCallback(async () => {
    await Promise.all([
      git.refresh(),
      tree.refresh(),
      editor.reloadAllOpenTabs(),
      project.refreshBranch(),
    ]);
  }, [editor.reloadAllOpenTabs, git, project.refreshBranch, tree]);

  const performCheckout = useCallback(
    async (name: string, discardChanges: boolean, isRemote: boolean) => {
      const ok = isRemote
        ? await branches.checkoutRemoteBranch(name, discardChanges)
        : await branches.checkoutBranch(name, discardChanges);
      if (!ok) return;
      project.setBranchFromGit(isRemote ? localNameFromRemoteBranch(name) : name);
      await refreshAfterBranchChange();
    },
    [branches, project.setBranchFromGit, refreshAfterBranchChange],
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

  const handleCheckoutBranch = useCallback(
    async (branch: GitBranchInfo) => {
      const saved = await editor.saveAllDirtyTabs();
      if (!saved) {
        setGitAlert({
          message: "Не удалось сохранить открытые файлы перед переключением ветки.",
        });
        return;
      }
      if (hasTrackedGitChanges(git.status)) {
        setBranchSwitchBlocked({
          kind: "checkout",
          branchName: branch.name,
          isRemote: branch.isRemote,
        });
        return;
      }
      await performCheckout(branch.name, false, branch.isRemote);
    },
    [editor.saveAllDirtyTabs, git.status, performCheckout],
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
        setBranchSwitchBlocked({ kind: "create", branchName: name });
        return;
      }
      await performCreateBranch(name, false);
    },
    [editor.saveAllDirtyTabs, git.status, performCreateBranch],
  );

  const handleDiscardAndSwitchBranch = useCallback(async () => {
    if (!branchSwitchBlocked) return;
    const { kind, branchName, isRemote } = branchSwitchBlocked;
    setBranchSwitchBlocked(null);
    if (kind === "checkout") {
      await performCheckout(branchName, true, isRemote ?? false);
    } else {
      await performCreateBranch(branchName, true);
    }
  }, [branchSwitchBlocked, performCheckout, performCreateBranch]);

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
      const ok = await git.discardFileChanges(repoRelativePath);
      if (!ok) return false;
      await syncEditorAfterGitDiscard(repoRelativePath);
      setGitDiffTarget(null);
      return true;
    },
    [git, syncEditorAfterGitDiscard],
  );

  const openDiagnostic = useCallback(
    async (documentId: string, line: number, column: number) => {
      // Если Problems panel был свёрнут — раскрываем его (как в IDE: клик по
      // проблеме не должен прятать сам список).
      if (!layout.bottomTool) {
        layout.setBottomToolId("problems");
      }

      const severity: "error" | "warning" = (() => {
        const found = workspaceIndex.diagnostics.find(
          (d) =>
            d.document === documentId &&
            d.line === line &&
            d.column === column,
        );
        return found?.severity === "warning" ? "warning" : "error";
      })();

      const reveal = () => {
        revealCounter.current += 1;
        setRevealRequest({
          id: revealCounter.current,
          line,
          column,
          severity,
        });
      };

      if (project.docsRoot && project.repoRoot) {
        // `documentId` — это repo-relative ключ индекса (например
        // `src/docs/asciidoc/foo.adoc`), а `editor.openFile` ожидает путь
        // относительно docsRoot (`foo.adoc`). Считаем относительный суффикс.
        const rel = toDocsRelativePath(
          documentId,
          project.repoRoot,
          project.docsRoot,
        );
        try {
          await editor.openFile(rel);
          reveal();
          return;
        } catch {
          // Путь не открылся — ниже общий fallback.
        }
      }
      try {
        await editor.openFile(documentId);
        reveal();
      } catch {
        // Файл не существует (битый include) — тихо игнорируем, не показывая
        // сырую os-ошибку. Пользователь уже видит диагностику в Problems.
      }
    },
    [
      editor,
      layout,
      project.docsRoot,
      project.repoRoot,
      workspaceIndex.diagnostics,
    ],
  );

  /**
   * Клик по xref-ссылке в превью AsciiDoc. Распарсивает href (`path#anchor`,
   * `path`, `#anchor`), открывает целевой файл и (если есть якорь)
   * прокручивает редактор к строке якоря через `findAnchors`.
   */
  const openXref = useCallback(
    async (href: string) => {
      const sourcePath = editor.activeTab?.path;
      if (!sourcePath) return;
      // Внешние URL — не наша зона ответственности, пропускаем.
      if (/^https?:\/\//i.test(href) || href.startsWith("mailto:")) return;

      const { path: relPath, anchor } = resolveXrefHref(href, sourcePath);

      try {
        await editor.openFile(relPath);
      } catch {
        // Файл не существует (битый xref) — тихо игнорируем; диагностику
        // пользователь уже видит в Problems, если она была построена.
        return;
      }

      if (!anchor) return;
      const repoRoot = project.repoRoot;
      const docsRoot = project.docsRoot;
      if (!repoRoot || !docsRoot) return;

      const documentId = toRepoRelativePath(relPath, repoRoot, docsRoot);
      try {
        const anchors = await findAnchors(documentId);
        const hit = anchors.find((a) => a.id === anchor);
        if (!hit) return;
        revealCounter.current += 1;
        setRevealRequest({
          id: revealCounter.current,
          line: hit.line,
          column: hit.column,
          severity: "warning",
        });
      } catch {
        // Индекс недоступен или документ не проиндексирован — оставляем
        // пользователя на открытой файле без прокрутки.
      }
    },
    [editor, project.repoRoot, project.docsRoot],
  );

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        if (hasProject) {
          void editor.saveActive().then((ok) => {
            if (ok) git.scheduleRefresh();
          });
        }
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.altKey) {
        if (event.key === "ArrowLeft") {
          event.preventDefault();
          void editor.goBack();
          return;
        }
        if (event.key === "ArrowRight") {
          event.preventDefault();
          void editor.goForward();
          return;
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [editor.saveActive, editor.goBack, editor.goForward, git.scheduleRefresh, hasProject]);

  // After autosave clears dirty flags, refresh Git status for gutter and panel.
  useEffect(() => {
    const dirtyCount = editor.tabs.filter((t) => t.dirty).length;
    if (
      prevDirtyCount.current > 0 &&
      dirtyCount < prevDirtyCount.current
    ) {
      git.scheduleRefresh();
    }
    prevDirtyCount.current = dirtyCount;
  }, [editor.tabs, git.scheduleRefresh]);

  const activePath = editor.activeTab?.path ?? null;
  const statusPath = activePath ?? "—";
  const statusFormat = activePath ? formatLabelFor(activePath) : "—";
  const statusLineEnding = editor.activeTab
    ? lineEndingLabelFor(editor.activeTab.content)
    : "—";

  // Диагностики приходят из бэкенда с `document` в repo-relative виде
  // (`src/docs/asciidoc/foo.adoc`), а активный таб хранит docs-relative
  // путь (`foo.adoc`). Приводим к единому виду для маркеров/подсветки.
  const editorDiagnostics = useMemo(() => {
    if (!project.docsRoot || !project.repoRoot) {
      return workspaceIndex.diagnostics;
    }
    return workspaceIndex.diagnostics.map((d) => ({
      ...d,
      document: toDocsRelativePath(d.document, project.repoRoot!, project.docsRoot!),
    }));
  }, [workspaceIndex.diagnostics, project.docsRoot, project.repoRoot]);

  const activeDocumentId = useMemo(() => {
    if (!activePath || !project.docsRoot || !project.repoRoot) return null;
    return toRepoRelativePath(activePath, project.repoRoot, project.docsRoot);
  }, [activePath, project.docsRoot, project.repoRoot]);

  const openProblems = useCallback(() => {
    layout.setBottomToolId("problems");
  }, [layout]);

  const canInsertAsciiDoc = Boolean(
    editor.activeTab && isAsciiDocPath(editor.activeTab.path),
  );

  const handleInsertSnippet = useCallback((text: string) => {
    const tabId = editor.activeTabId;
    if (!tabId) return;
    insertCounter.current += 1;
    setInsertRequest({ id: insertCounter.current, tabId, text });
  }, [editor.activeTabId]);

  const toastMessage = editor.error ?? folderError;
  const visibleToastMessage =
    toastMessage && toastMessage !== dismissedToastMessage ? toastMessage : null;

  useEffect(() => {
    if (!toastMessage) return;
    const timer = setTimeout(() => {
      setDismissedToastMessage(toastMessage);
    }, 3000);
    return () => clearTimeout(timer);
  }, [toastMessage]);

  return (
    <div className="app" style={panelStyle}>
      <TopBar
        repoName={project.projectName ?? "—"}
        branchName={project.branchName ?? "—"}
        projectRoot={project.repoRoot}
        hasProject={hasProject}
        gitBusy={git.busy}
        branchesPanelOpen={layout.activeTool === "branches"}
        branchBusy={branches.busy}
        onBranchChipClick={() => layout.setRightTool("branches")}
        onOpenFolder={openFolder}
        onCloseProject={closeProject}
        onSave={async () => {
          const ok = await editor.saveActive();
          if (ok) git.scheduleRefresh();
          return ok;
        }}
        onPrefsChange={generalPrefs.setPrefs}
        onToggleSidebar={layout.toggleSidebar}
        onToggleRight={toggleRightPanel}
        onToggleBottom={toggleBottomPanel}
        onToggleGit={toggleGitPanel}
        onOpenBranches={() => layout.setRightTool(layout.activeTool === "branches" ? null : "branches")}
        onPull={openPullModal}
        onPush={() => void runPush()}
        onGoBack={() => void editor.goBack()}
        onGoForward={() => void editor.goForward()}
        canGoBack={editor.canGoBack}
        canGoForward={editor.canGoForward}
        onSelectProject={async (root) => {
          await closeProject();
          try {
            await project.beginOpenPath(root);
          } catch (e) {
            setFolderError(e instanceof Error ? e.message : String(e));
          }
        }}
        onCloneProject={async (cloned) => {
          project.submitProbe(cloned);
        }}
      />
      <div className="workspace">
        <div className={mainClassName}>
          <Sidebar
            open={layout.sidebarOpen}
            onToggle={layout.toggleSidebar}
            docsRoot={project.docsRoot}
            tree={tree.nodes}
            treeLoading={tree.loading}
            treeError={tree.error}
            activePath={editor.activeTab?.path ?? null}
            expandedDirs={session.expandedDirs}
            separateExternal={generalPrefs.prefs.separateExternalFolder}
            onToggleDir={session.toggleDir}
            onOpenFile={(path) => void editor.openFile(path)}
            onNewFile={setNewFileParent}
            onNewFolder={setNewFolderParent}
            onRename={setRenameTarget}
            onDelete={setDeleteTarget}
            onMove={async (source, destDirPath) => {
              if (!project.docsRoot) return;
              const oldPath = source.path;
              const name = oldPath.split(/[/\\]/).filter(Boolean).pop();
              if (!name) return;
              const newPath = joinParent(destDirPath, name);
              try {
                if (source.isDir) {
                  await renameProjectDir(project.docsRoot, oldPath, newPath);
                } else {
                  await renameProjectFile(project.docsRoot, oldPath, newPath);
                }
              } catch (e) {
                setFolderError(e instanceof Error ? e.message : String(e));
                return;
              }
              editor.remapTabsUnder(oldPath, newPath);
              session.remapExpandedUnder(oldPath, newPath);
              session.ensureExpanded(destDirPath);
              await tree.refresh();
              git.scheduleRefresh();
            }}
            onCopy={async (target) => {
              if (!target.isDir && project.docsRoot) {
                try {
                  const content = await readProjectFile(project.docsRoot, target.path);
                  await writeText(content);
                } catch {
                  // системный буфер недоступен — внутреннее копирование всё равно работает
                }
              }
              setCopiedItem(target);
            }}
            copiedItem={copiedItem}
            onPaste={async (destDirPath) => {
              if (!project.docsRoot || !copiedItem) return;
              const name = copiedItem.path.split(/[/\\]/).filter(Boolean).pop();
              if (!name) return;
              const newPath = joinParent(
                destDirPath,
                withCopySuffix(name, copiedItem.isDir),
              );
              try {
                if (copiedItem.isDir) {
                  await copyProjectDir(project.docsRoot, copiedItem.path, newPath);
                } else {
                  await copyProjectFile(project.docsRoot, copiedItem.path, newPath);
                }
              } catch (e) {
                setFolderError(e instanceof Error ? e.message : String(e));
                return;
              }
              session.ensureExpanded(destDirPath);
              await tree.refresh();
              git.scheduleRefresh();
            }}
            onResize={panels.resizeSidebarBy}
            onResizeEnd={panels.persistLayout}
            onResizeExternal={panels.resizeExternalBy}
            onResizeExternalEnd={panels.persistLayout}
          />
          {hasProject ? (
            <EditorPane
              tabs={editor.tabs}
              activeTabId={editor.activeTabId}
              activeTab={editor.activeTab}
              onSelectTab={editor.selectTab}
              onCloseTab={editor.closeTab}
              onCloseAllTabs={editor.closeAllTabs}
              onCloseOtherTabs={editor.closeOtherTabs}
              onChangeContent={editor.updateActiveContent}
              onCursorChange={editor.setCursor}
              diagnostics={editorDiagnostics}
              completionsEnabled={workspaceIndex.status !== "idle"}
              revealRequest={revealRequest}
              insertRequest={insertRequest}
              onOpenProblems={openProblems}
              onOpenXref={openXref}
              viewMode={viewMode.viewMode}
              onViewModeChange={viewMode.setViewMode}
              docsRoot={project.docsRoot}
              gitGutter={
                project.repoRoot && project.docsRoot
                  ? {
                      repoRoot: project.repoRoot,
                      docsRoot: project.docsRoot,
                      loadFileDiff: git.loadFileDiff,
                    }
                  : null
              }
              editorFontSizePx={generalPrefs.prefs.editorFontSizePx}
            />
          ) : (
            <Welcome
              onOpenFolder={openFolder}
              onOpenRecent={openRecent}
              onCloneProject={async (cloned) => {
                project.submitProbe(cloned);
              }}
              error={project.ready ? (folderError ?? project.error) : null}
            />
          )}
          <RightDock
            activeTool={layout.activeTool}
            onToggleTool={layout.toggleRightTool}
            onHide={() => layout.setRightTool(null)}
            onResize={panels.resizeRightBy}
            onResizeEnd={panels.persistLayout}
            git={
              hasProject
                ? {
                    staged: git.status.staged,
                    unstaged: git.status.unstaged,
                    commits: git.commits,
                    jiraKey: git.jiraKey,
                    onJiraKeyChange: git.setJiraKey,
                    description: git.description,
                    onDescriptionChange: git.setDescription,
                    canCommit: git.canCommit,
                    busy: git.busy,
                    error: git.error,
                    onStage: (path) => void git.stage([path]),
                    onUnstage: (path) => void git.unstage([path]),
                    onStageAll: (paths) => void git.stage(paths),
                    onUnstageAll: () =>
                      void git.unstage(git.status.staged.map((f) => f.path)),
                    onCommit: () => void git.commit(),
                    onRefresh: () => void git.refresh(),
                    onOpenFileDiff: openGitFileDiff,
                    selectedDiff: gitDiffTarget
                      ? {
                          path: gitDiffTarget.file.path,
                          scope: gitDiffTarget.scope,
                        }
                      : null,
                  }
                : null
            }
            asciidoc={
              hasProject
                ? {
                    canInsert: canInsertAsciiDoc,
                    onInsert: handleInsertSnippet,
                  }
                : null
            }
            branches={
              hasProject
                ? {
                    currentBranch: project.branchName ?? "—",
                    branches: branches.branches,
                    busy: branches.busy,
                    error: branches.error,
                    onCheckout: (branch) => void handleCheckoutBranch(branch),
                    onCreateBranch: (name) => void handleCreateBranch(name),
                    onRefresh: () => void branches.refresh(),
                    onFetch: () => void branches.fetchBranches(),
                  }
                : null
            }
          />
        </div>
        <BottomDock
          activeTool={layout.bottomTool}
          onToggleTool={layout.toggleBottomTool}
          onHide={() => layout.setBottomToolId(null)}
          onResize={panels.resizeBottomBy}
          onResizeEnd={panels.persistLayout}
          diagnostics={workspaceIndex.diagnostics}
          activeDocumentId={activeDocumentId}
          onOpenDiagnostic={(documentId, line, column) =>
            void openDiagnostic(documentId, line, column)
          }
        />
      </div>
      <StatusBar
        filePath={hasProject ? statusPath : "—"}
        formatLabel={statusFormat}
        lineEndingLabel={statusLineEnding}
        cursorLabel={cursorLabel}
        hasActiveFile={Boolean(hasProject && activePath)}
        indexStatus={workspaceIndex.status}
        indexProgress={workspaceIndex.progress}
        indexStats={workspaceIndex.stats}
      />

      {project.pendingOpen ? (
        <ConfirmOpenProjectModal
          probe={project.pendingOpen}
          onCancel={project.cancelPendingOpen}
          onConfirm={async (docsRoot) => {
            await project.confirmPendingOpen(docsRoot);
          }}
        />
      ) : null}

      {newFileParent !== null && project.docsRoot ? (
        <NewFileModal
          parentPath={newFileParent}
          onCancel={() => setNewFileParent(null)}
          onConfirm={async (fileName) => {
            const relativePath = joinParent(newFileParent, fileName);
            await createProjectFile(project.docsRoot!, relativePath);
            session.ensureExpanded(newFileParent);
            setNewFileParent(null);
            await tree.refresh();
            await editor.openFile(relativePath);
          }}
        />
      ) : null}

      {newFolderParent !== null && project.docsRoot ? (
        <NewFolderModal
          parentPath={newFolderParent}
          onCancel={() => setNewFolderParent(null)}
          onConfirm={async (folderName) => {
            const relativePath = joinParent(newFolderParent, folderName);
            await createProjectDir(project.docsRoot!, relativePath);
            session.ensureExpanded(relativePath);
            setNewFolderParent(null);
            await tree.refresh();
          }}
        />
      ) : null}

      {deleteTarget !== null && project.docsRoot ? (
        <DeleteConfirmModal
          target={deleteTarget}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={async (target) => {
            if (target.isDir) {
              await deleteProjectDir(project.docsRoot!, target.path);
            } else {
              await deleteProjectFile(project.docsRoot!, target.path);
            }
            editor.discardTabsUnder(target.path);
            setDeleteTarget(null);
            await tree.refresh();
            git.scheduleRefresh();
          }}
        />
      ) : null}

      {renameTarget !== null && project.docsRoot ? (
        <RenameModal
          target={renameTarget}
          onCancel={() => setRenameTarget(null)}
          onConfirm={async (newName) => {
            const oldPath = renameTarget.path;
            const newPath = joinParent(parentOfPath(oldPath), newName);
            if (renameTarget.isDir) {
              await renameProjectDir(project.docsRoot!, oldPath, newPath);
            } else {
              await renameProjectFile(project.docsRoot!, oldPath, newPath);
            }
            editor.remapTabsUnder(oldPath, newPath);
            session.remapExpandedUnder(oldPath, newPath);
            session.ensureExpanded(parentOfPath(newPath));
            setRenameTarget(null);
            await tree.refresh();
            git.scheduleRefresh();
          }}
        />
      ) : null}

      {pullModalOpen ? (
        <PullUpdateModal
          busy={git.busy}
          onCancel={() => setPullModalOpen(false)}
          onConfirm={(mode) => void onPullConfirm(mode)}
          onRequestResetToRemote={() => setResetRemoteConfirmOpen(true)}
        />
      ) : null}

      {resetRemoteConfirmOpen ? (
        <ResetRemoteConfirmModal
          busy={git.busy}
          onCancel={() => setResetRemoteConfirmOpen(false)}
          onConfirm={() => void onResetToRemoteConfirm()}
        />
      ) : null}

      {branchSwitchBlocked ? (
        <CheckoutBlockedModal
          branchName={branchSwitchBlocked.branchName}
          mode={branchSwitchBlocked.kind}
          busy={branches.busy || git.busy}
          onCancel={() => setBranchSwitchBlocked(null)}
          onOpenCommit={() => {
            layout.setRightTool("git");
            setBranchSwitchBlocked(null);
          }}
          onDiscardAndContinue={() => void handleDiscardAndSwitchBranch()}
        />
      ) : null}

      {gitDiffTarget ? (
        <GitFileDiffModal
          target={gitDiffTarget}
          busy={git.busy}
          editorFontSizePx={generalPrefs.prefs.editorFontSizePx}
          onClose={() => setGitDiffTarget(null)}
          onLoadDiff={git.loadFileDiff}
          onDiscard={handleGitDiscard}
        />
      ) : null}

      {gitAlert ? (
        <AlertOkModal
          title={gitAlert.title}
          message={gitAlert.message}
          variant={gitAlert.variant}
          onClose={() => setGitAlert(null)}
        />
      ) : null}

      {visibleToastMessage ? (
        <div className="app-toast" role="status">
          <span className="app-toast-message">{visibleToastMessage}</span>
          <button
            type="button"
            className="app-toast-close"
            onClick={() => setDismissedToastMessage(toastMessage)}
            aria-label="Закрыть"
          >
            ×
          </button>
        </div>
      ) : null}
    </div>
  );
}

export default App;
