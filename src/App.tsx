import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type * as Monaco from "monaco-editor";
import { toMessage } from "./lib/errors";
import { BottomDock } from "./components/BottomDock/BottomDock";
import { EditorPane } from "./components/Editor/Editor";
import { AlertOkModal } from "./components/Git/AlertOkModal";
import { GitConflictModal } from "./components/Git/GitConflictModal";
import { GitFileDiffModal } from "./components/Git/GitFileDiffModal";
import { GitCommitFileDiffModal } from "./components/Git/GitCommitFileDiffModal";
import { CheckoutBlockedModal } from "./components/Git/CheckoutBlockedModal";
import { PullUpdateModal } from "./components/Git/PullUpdateModal";
import { PushConfirmModal } from "./components/Git/PushConfirmModal";
import { ResetRemoteConfirmModal } from "./components/Git/ResetRemoteConfirmModal";
import { DeleteBranchConfirmModal } from "./components/Git/DeleteBranchConfirmModal";
import { DiscardStashConfirmModal } from "./components/Git/DiscardStashConfirmModal";
import { GitStashPreviewModal } from "./components/Git/GitStashPreviewModal";
import { RightDock } from "./components/RightDock/RightDock";
import { DocsSearchOverlay } from "./components/Search/DocsSearchOverlay";
import { NewFileModal } from "./components/Sidebar/NewFileModal";
import { NewFolderModal } from "./components/Sidebar/NewFolderModal";
import { DeleteConfirmModal } from "./components/Sidebar/DeleteConfirmModal";
import { Sidebar } from "./components/Sidebar/Sidebar";
import { StatusBar } from "./components/StatusBar/StatusBar";
import { TopBar } from "./components/TopBar/TopBar";
import { ConfirmOpenProjectModal } from "./components/Welcome/ConfirmOpenProjectModal";
import { Welcome } from "./components/Welcome/Welcome";
import { friendlyGitError } from "./lib/gitErrors";
import { useBranches } from "./hooks/useBranches";
import { useGitStash } from "./hooks/useGitStash";
import { useGitActionLog } from "./hooks/useGitActionLog";
import { useDocsTree } from "./hooks/useDocsTree";
import { useDocNavigation } from "./hooks/useDocNavigation";
import { useFileTreeActions } from "./hooks/useFileTreeActions";
import { useEditorTabActions } from "./hooks/useEditorTabActions";
import { useEditorTabs } from "./hooks/useEditorTabs";
import { useSpecsRepo } from "./hooks/useSpecsRepo";
import { useOpenApiBundle } from "./hooks/useOpenApiBundle";
import { OpenApiExplorer } from "./components/OpenApiExplorer/OpenApiExplorer";
import { useGeneralPrefs } from "./hooks/useGeneralPrefs";
import { useSpellcheckConfig } from "./hooks/useSpellcheckConfig";
import { useGitPanel } from "./hooks/useGitPanel";
import { useGitWorkflow } from "./hooks/useGitWorkflow";
import { usePanelLayout } from "./hooks/usePanelLayout";
import { useProject } from "./hooks/useProject";
import { useWindowTitle } from "./hooks/useWindowTitle";
import { useAsciiDocParser } from "./hooks/useAsciiDocParser";
import { useEditorViewMode } from "./hooks/useEditorViewMode";
import { useWorkspaceIndex } from "./hooks/useWorkspaceIndex";
import { useStandardsCheck } from "./hooks/useStandardsCheck";
import { useEmbeddingIndexWarmup } from "./hooks/useEmbeddingIndexWarmup";
import { useEmbeddingPriorityFiles } from "./hooks/useEmbeddingPriorityFiles";
import { useEmbeddingSetup } from "./hooks/useEmbeddingSetup";
import { useLlmSetup } from "./hooks/useLlmSetup";
import { useLlmRateLimit } from "./hooks/useLlmRateLimit";
import { useAppToasts } from "./hooks/useAppToasts";
import { useWorkspaceLayout } from "./hooks/useWorkspaceLayout";
import {
  collectDirPaths,
  useWorkspaceSession,
} from "./hooks/useWorkspaceSession";
import {
  copyProjectDir,
  copyProjectFile,
  createProjectDir,
  createProjectFileFromTemplate,
  createRestEndpointFolder,
  deleteProjectDir,
  deleteProjectFile,
  readProjectFile,
  renameProjectDir,
  renameProjectFile,
  type UpdatedReference,
} from "./lib/project";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import {
  formatLabelFor,
  isAsciiDocPath,
  lineEndingLabelFor,
} from "./lib/supportedFiles";
import {
  toDocsRelativePath,
  toRepoRelativePath,
} from "./lib/paths";
import { RenameModal } from "./components/Sidebar/RenameModal";
import { useOsFileDrop } from "./hooks/useOsFileDrop";

function joinParent(parentPath: string, name: string): string {
  if (!parentPath || parentPath === ".") return name;
  return `${parentPath.replace(/[/\\]+$/, "")}/${name}`;
}

function dirnameOf(path: string): string {
  const segments = path.split(/[/\\]/).filter(Boolean);
  segments.pop();
  return segments.length > 0 ? segments.join("/") : ".";
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

function parentOfPath(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  if (parts.length <= 1) return ".";
  return parts.slice(0, -1).join("/");
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
  useWindowTitle(project.projectName);
  const generalPrefs = useGeneralPrefs();
  const spellcheck = useSpellcheckConfig();
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

  const specsRepo = useSpecsRepo(project.repoRoot);
  const tabs = useEditorTabActions({ editor, specsRepo });
  const {
    openApiTabOpen,
    setOpenApiTabOpen,
    activeKind,
    setActiveKind,
    displayTabs,
    openApiExplorerTab,
  } = tabs;

  const openApiBundle = useOpenApiBundle(
    project.repoRoot,
    specsRepo.info?.entryFile ?? null,
    openApiTabOpen,
  );


  useEffect(() => {
    const onOpenPlan = (event: Event) => {
      const planId = (event as CustomEvent<{ planId?: string }>).detail?.planId;
      if (!planId) return;
      void editor.openPlan(planId);
    };
    window.addEventListener("atlas-open-plan", onOpenPlan);
    return () => window.removeEventListener("atlas-open-plan", onOpenPlan);
  }, [editor.openPlan]);








  // State (not a ref) — the Ctrl+Click editor-opener registration effect
  // below needs to re-run once this becomes available.
  const [monacoInstance, setMonacoInstance] = useState<typeof Monaco | null>(null);
  const git = useGitPanel(project.repoRoot, {
    active: Boolean(project.repoRoot),
    onBranchChange: project.setBranchFromGit,
  });
  const branches = useBranches(project.repoRoot, {
    active: Boolean(project.repoRoot),
  });
  const stash = useGitStash(project.repoRoot, {
    active: Boolean(project.repoRoot),
  });
  const actionLog = useGitActionLog(project.repoRoot, {
    active: Boolean(project.repoRoot),
  });
  const { toast: activeToast, showSuccess, folderError, setFolderError } =
    useAppToasts(editor.error);
  const fileTree = useFileTreeActions({
    project,
    tree,
    session,
    editor,
    git,
    showSuccess,
    setError: setFolderError,
  });
  const {
    newFileParent,
    setNewFileParent,
    newFolderParent,
    setNewFolderParent,
    deleteTarget,
    setDeleteTarget,
    copiedItem,
    setCopiedItem,
    renameTarget,
    setRenameTarget,
    applyRenameReport,
  } = fileTree;

  const [docsSearchOpen, setDocsSearchOpen] = useState(false);
  const [insertRequest, setInsertRequest] = useState<{
    id: number;
    tabId: string;
    text: string;
  } | null>(null);
  // «Добавить в чат» из панели выделения — тот же паттерн «запрос с id», что и
  // insertRequest выше: id нужен, чтобы повторная вставка того же текста не
  // проглатывалась проверкой на равенство пропсов.
  const [chatInsertRequest, setChatInsertRequest] = useState<{
    id: number;
    text: string;
    filePath: string | null;
  } | null>(null);
  const insertCounter = useRef(0);
  const chatInsertCounter = useRef(0);
  const skipNextPanelSync = useRef(false);
  const prevDirtyCount = useRef(0);
  const seededDocsRoot = useRef<string | null>(null);

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
    if (seededDocsRoot.current === project.docsRoot) return;
    seededDocsRoot.current = project.docsRoot;
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


  const {
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
    pendingStashConflict,
    pullModalOpen,
    pushConfirmOpen,
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
  } = useGitWorkflow({
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
  });


  const { osDropTargetPath } = useOsFileDrop(hasProject, {
    onImportExternal: (destDirPath, paths) => {
      void fileTree.importExternal(destDirPath, paths);
    },
    onOpenExternal: (absolutePath) => {
      void editor.openExternalFile(absolutePath);
    },
    onReject: (message) => {
      setFolderError(message);
    },
  });

  const workspaceIndex = useWorkspaceIndex(project.repoRoot, {
    active: hasProject,
  });
  const {
    revealRequest,
    openDiagnostic,
    openDocsSearchHit,
    openDocumentReference,
    openXref,
  } = useDocNavigation({ editor, project, layout, workspaceIndex, monacoInstance });
  const standards = useStandardsCheck(project.docsRoot, {
    active: hasProject,
  });
  useEmbeddingIndexWarmup(project.repoRoot, { active: hasProject });
  // Feeds the status bar's embedding-index segment — mostly a read-only
  // observer of whatever `AssistantPanel`/`EmbeddingsTab`'s own instances
  // (or the incremental file watcher) triggered, except for the segment's
  // own click-to-sync handler below, which calls this instance's `sync()`
  // directly. See those hooks' doc comment on why each panel keeps its own
  // separate instance rather than sharing one.
  const embeddingSetup = useEmbeddingSetup(project.repoRoot);
  const llmSetup = useLlmSetup();
  const selectionAiProviderId =
    llmSetup.settings?.activeProviderId ?? llmSetup.providers[0]?.id ?? null;
  // Backend snapshot already respects `rateLimitEnabled` and the baked-in
  // preset; gating here on App's copy of settings would miss a toggle
  // made in the Settings dialog (a separate `useLlmSetup` instance).
  const rateLimit = useLlmRateLimit(selectionAiProviderId);
  const selectionAiLlmReady =
    selectionAiProviderId !== null &&
    Boolean(llmSetup.hasApiKeyMap[selectionAiProviderId]);
  // Shows a brief "Синхронизировано" confirmation in the status bar segment
  // right after a successful click-to-sync — `indexStatus` alone would just
  // silently settle back to the normal "Проиндексировано чанков: N" label,
  // which doesn't read as clear feedback that the click actually did
  // something. `stats === null` means `sync()` caught an error internally
  // (it never rejects — see `useEmbeddingSetup.ts`), so this only fires on
  // a real success.
  const [embedJustSynced, setEmbedJustSynced] = useState(false);
  const handleEmbedSyncClick = () => {
    void embeddingSetup.sync().then((stats) => {
      if (!stats) return;
      setEmbedJustSynced(true);
      setTimeout(() => setEmbedJustSynced(false), 1500);
    });
  };
  useEmbeddingPriorityFiles(
    editor.tabs.filter((t) => t.origin === "project").map((t) => t.path),
    { active: hasProject },
  );
  const [standardsSettingsSignal, setStandardsSettingsSignal] = useState(0);
  const [llmSettingsSignal, setLlmSettingsSignal] = useState(0);





  const cursorLabel = hasProject
    ? `Ln ${editor.cursor.line}, Col ${editor.cursor.column}`
    : "Ln 1, Col 1";









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
      setFolderError(toMessage(e));
    }
  }, [project]);

  const openRecent = useCallback(
    async (root: string) => {
      setFolderError(null);
      try {
        await project.beginOpenPath(root);
      } catch (e) {
        setFolderError(toMessage(e));
        throw e;
      }
    },
    [project],
  );

  const closeProject = useCallback(async () => {
    await editor.reset();
    await project.closeProject();
    setOpenApiTabOpen(false);
    setActiveKind("file");
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
      layout.setBottomToolId("gitHistory");
    }
  }, [layout]);

  const toggleGitPanel = useCallback(() => {
    layout.toggleRightTool("git");
  }, [layout]);



























  // Reconciles an open editor tab against a change the AI assistant just
  // made directly on disk (`writeFile`/`editFile`/`deleteFile`/
  // `createDirectory`/`deleteDirectory`, see `AssistantConversation`'s
  // `onFileWritten`). Without this, a tab left open on the affected path
  // keeps showing its now-stale in-memory content — and if it's dirty (or
  // becomes dirty) by the time autosave/save-on-switch fires,
  // `useEditorTabs.saveTab` would write that stale content straight back to
  // disk, silently reverting the assistant's change (or resurrecting a file
  // it just deleted). Mirrors `editor.discardTabsUnder` in the manual
  // tree-delete confirm above for the delete case, and `reloadTabFromDisk`
  // in `applyRenameReport`/`handleGitSaveContent` for the write case.
  const handleAssistantFileWritten = useCallback(
    ({ tool, path }: { tool: string; path: string }) => {
      void tree.refresh();
      if (openApiBundle.bundle) void openApiBundle.reload();
      // Tool results use access-mode-relative paths; editor tabs are docs-relative.
      const docsPath =
        project.repoRoot && project.docsRoot
          ? toDocsRelativePath(path, project.repoRoot, project.docsRoot)
          : path;
      switch (tool) {
        case "writeFile":
        case "editFile":
          void editor.reloadTabFromDisk(docsPath);
          break;
        case "deleteFile":
        case "deleteDirectory":
          editor.discardTabsUnder(docsPath);
          break;
        default:
          break;
      }
    },
    [tree, editor, openApiBundle, project.repoRoot, project.docsRoot],
  );

  // Same idea as `handleAssistantFileWritten`, but a `move` tool call has
  // both an old and a new path, so a plain reload isn't enough — an open
  // tab under `from` needs to keep pointing at the same file at its new
  // path, exactly like the manual drag-and-drop `onMove` handler below
  // achieves via `editor.remapTabsUnder`/`session.remapExpandedUnder`. The
  // move's reference-rewrite report reuses `applyRenameReport` so cascaded
  // changes to *other* open tabs (files that included/referenced the moved
  // one) get reloaded and reported the same way a manual rename does.
  const handleAssistantFileMoved = useCallback(
    ({ from, to, updatedFiles }: { from: string; to: string; updatedFiles: UpdatedReference[] }) => {
      const toDocs = (p: string) =>
        project.repoRoot && project.docsRoot
          ? toDocsRelativePath(p, project.repoRoot, project.docsRoot)
          : p;
      const docsFrom = toDocs(from);
      const docsTo = toDocs(to);
      editor.remapTabsUnder(docsFrom, docsTo);
      session.remapExpandedUnder(docsFrom, docsTo);
      session.ensureExpanded(dirnameOf(docsTo));
      void tree.refresh();
      if (openApiBundle.bundle) void openApiBundle.reload();
      git.scheduleRefresh();
      void applyRenameReport({
        updatedFiles: updatedFiles.map((f) => ({
          docsRelativePath: toDocs(f.docsRelativePath),
          count: f.count,
        })),
      });
    },
    [editor, session, tree, git, applyRenameReport, openApiBundle, project.repoRoot, project.docsRoot],
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
      if (
        (event.metaKey || event.ctrlKey) &&
        event.shiftKey &&
        event.key.toLowerCase() === "f"
      ) {
        event.preventDefault();
        if (hasProject) setDocsSearchOpen(true);
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

  // Открывает док «Ассистент» (если закрыт) и кладёт выделенный фрагмент в
  // черновик ввода чата — AssistantConversation подхватит запрос по id.
  const handleAddSelectionToChat = useCallback(
    (text: string, filePath: string | null) => {
      chatInsertCounter.current += 1;
      layout.setRightTool("assistant");
      setChatInsertRequest({ id: chatInsertCounter.current, text, filePath });
    },
    [layout],
  );

  // AssistantConversation вызывает это сразу после того, как вставит запрос
  // в черновик — очищает запрос здесь, в App, а не только локальным флагом
  // «уже обработано» внутри AssistantConversation, потому что тот компонент
  // перемонтируется (смена чата, переключение инструментов дока) и терял бы
  // свою локальную отметку, повторно вставляя тот же запрос в новый черновик.
  const handleChatInsertHandled = useCallback(() => {
    setChatInsertRequest(null);
  }, []);

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
        onBranchChipClick={() =>
          layout.setRightTool(layout.activeTool === "branches" ? null : "branches")
        }
        onOpenFolder={openFolder}
        onCloseProject={closeProject}
        onSave={async () => {
          const ok = await editor.saveActive();
          if (ok) git.scheduleRefresh();
          return ok;
        }}
        onUndo={tabs.undo}
        onRedo={tabs.redo}
        hasActiveTab={editor.activeTab !== null}
        onPrefsChange={generalPrefs.setPrefs}
        onSpellcheckConfigChange={spellcheck.setConfig}
        onToggleSidebar={layout.toggleSidebar}
        onToggleRight={toggleRightPanel}
        onToggleBottom={toggleBottomPanel}
        onToggleGit={toggleGitPanel}
        onOpenBranches={() => layout.setRightTool(layout.activeTool === "branches" ? null : "branches")}
        onPull={openPullModal}
        onPush={() => void runPush()}
        onGoBack={() => void editor.goBack()}
        onGoForward={() => void editor.goForward()}
        onFindInDocs={() => setDocsSearchOpen(true)}
        canGoBack={editor.canGoBack}
        canGoForward={editor.canGoForward}
        syncPillState={syncPillState}
        onSyncPillClick={handleSyncPillClick}
        onSelectProject={async (root) => {
          await closeProject();
          try {
            await project.beginOpenPath(root);
          } catch (e) {
            setFolderError(toMessage(e));
          }
        }}
        onCloneProject={async (cloned) => {
          project.submitProbe(cloned);
        }}
        openStandardsSettingsSignal={standardsSettingsSignal}
        openLlmSettingsSignal={llmSettingsSignal}
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
            activePath={
              editor.activeTab?.origin === "project"
                ? editor.activeTab.path
                : null
            }
            expandedDirs={session.expandedDirs}
            separateExternal={generalPrefs.prefs.separateExternalFolder}
            specsRepo={specsRepo.info}
            onOpenApiExplorer={openApiExplorerTab}
            onToggleDir={session.toggleDir}
            onRefreshTree={() => {
              void tree.refresh();
              void workspaceIndex.rebuildIndex();
              if (openApiBundle.bundle) void openApiBundle.reload();
            }}
            onExpandAll={() => session.expandAll(collectDirPaths(tree.nodes))}
            onCollapseAll={session.collapseAll}
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
              let report: Awaited<ReturnType<typeof renameProjectFile>>;
              try {
                report = source.isDir
                  ? await renameProjectDir(project.docsRoot, oldPath, newPath)
                  : await renameProjectFile(project.docsRoot, oldPath, newPath);
              } catch (e) {
                setFolderError(toMessage(e));
                return;
              }
              editor.remapTabsUnder(oldPath, newPath);
              session.remapExpandedUnder(oldPath, newPath);
              session.ensureExpanded(destDirPath);
              await tree.refresh();
              git.scheduleRefresh();
              void applyRenameReport(report);
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
            osDropTargetPath={osDropTargetPath}
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
                setFolderError(toMessage(e));
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
              tabs={displayTabs}
              activeTabId={activeKind === "openapi" ? "openapi" : editor.activeTabId}
              activeTab={editor.activeTab}
              activeKind={activeKind}
              openApiExplorer={
                openApiTabOpen ? (
                  <OpenApiExplorer
                    bundle={openApiBundle.bundle}
                    loading={openApiBundle.loading}
                    error={openApiBundle.error}
                  />
                ) : undefined
              }
              onSelectTab={tabs.selectTab}
              onCloseTab={tabs.closeTab}
              onCloseAllTabs={tabs.closeAllTabs}
              onCloseOtherTabs={tabs.closeOtherTabs}
              onChangeContent={editor.updateActiveContent}
              onCursorChange={editor.setCursor}
              diagnostics={editorDiagnostics}
              completionsEnabled={workspaceIndex.status !== "idle"}
              spellcheckConfig={spellcheck.config}
              revealRequest={revealRequest}
              insertRequest={insertRequest}
              onOpenProblems={openProblems}
              onOpenXref={openXref}
              onOpenDocumentReference={openDocumentReference}
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
              providerId={selectionAiProviderId}
              llmReady={selectionAiLlmReady}
              onAddToChat={handleAddSelectionToChat}
              onEditorInstanceChange={tabs.onEditorInstanceChange}
              onMonacoInstanceChange={setMonacoInstance}
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
                    conflicted: git.status.conflicted,
                    mergeInProgress: git.status.mergeInProgress,
                    jiraKey: git.jiraKey,
                    onJiraKeyChange: git.setJiraKey,
                    description: git.description,
                    onDescriptionChange: git.setDescription,
                    canCommit: git.canCommit,
                    busy: git.busy,
                    error: git.error,
                    onStage: (path) => handleStage([path]),
                    onUnstage: (path) => handleUnstage([path]),
                    onStageAll: (paths) => handleStage(paths),
                    onUnstageAll: () =>
                      handleUnstage(git.status.staged.map((f) => f.path)),
                    onCommit: handleCommit,
                    onRefresh: () => void git.refresh(),
                    onOpenFileDiff: openGitFileDiff,
                    onOpenConflict: openConflict,
                    onAbortMerge: () => void onAbortMerge(),
                    onFinishMerge: () => void onFinishMergeRetry(),
                    selectedDiff: gitDiffTarget
                      ? {
                          path: gitDiffTarget.file.path,
                          scope: gitDiffTarget.scope,
                        }
                      : null,
                    shelf: stash.entries,
                    shelfBusy: stash.busy,
                    currentBranch: git.status.branch,
                    pendingShelfConflictId: pendingStashConflict?.id ?? null,
                    onRestoreShelfEntry: (entry) => void onRestoreShelfEntry(entry),
                    onDiscardShelfEntry,
                    onPreviewShelfEntry,
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
                    busy: branches.busy || embeddingSetup.busy,
                    error: branches.error,
                    onCheckout: (branch) => void handleCheckoutBranch(branch),
                    onCreateBranch: (name) => void handleCreateBranch(name),
                    onRefresh: () => void branches.refresh(),
                    onFetch: () => void branches.fetchBranches(),
                    onDelete: (branch) => setDeleteBranchTarget(branch),
                  }
                : null
            }
            assistant={{
              onOpenSettings: () => setLlmSettingsSignal((n) => n + 1),
              specsRepoInfo: specsRepo.info,
              docsRoot: project.docsRoot ?? "",
              onFileWritten: handleAssistantFileWritten,
              onFileMoved: handleAssistantFileMoved,
              repoRoot: project.repoRoot,
              activeFilePath: editor.activeTab?.path ?? null,
            }}
            chatInsertRequest={chatInsertRequest}
            onChatInsertHandled={handleChatInsertHandled}
            gitActionLog={
              hasProject
                ? {
                    entries: actionLog.entries,
                    busy: git.busy || branches.busy || stash.busy,
                    onUndo: (entry) => void handleUndoAction(entry),
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
          gitHistory={
            hasProject
              ? {
                  commits: git.commits,
                  busy: git.busy,
                  error: git.error,
                  onRefresh: () => void git.refresh(),
                  onLoadCommitFiles: loadCommitFiles,
                  onOpenCommitFileDiff: openCommitFileDiff,
                }
              : null
          }
          standardsReport={standards.report}
          standardsStatus={standards.status}
          standardsError={standards.error}
          standardsActiveDocsPath={activePath}
          onRunStandardsCheck={() => void standards.runCheck()}
          onOpenStandardsSettings={() =>
            setStandardsSettingsSignal((n) => n + 1)
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
        embedIndexStatus={hasProject ? embeddingSetup.indexStatus : null}
        embedSyncProgress={hasProject ? embeddingSetup.syncProgress : null}
        onEmbedSyncClick={hasProject ? handleEmbedSyncClick : undefined}
        embedSyncDisabled={embeddingSetup.busy || !embeddingSetup.providerConfigured}
        embedJustSynced={embedJustSynced}
        rateLimitSnapshot={rateLimit.snapshot}
        rateLimitPopoverOpen={rateLimit.popoverOpen}
        onRateLimitPopoverChange={rateLimit.setPopoverOpen}
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
          onConfirm={async (fileName, template) => {
            const relativePath = joinParent(newFileParent, fileName);
            await createProjectFileFromTemplate(
              project.docsRoot!,
              relativePath,
              template,
            );
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
          onConfirm={async (folderName, useRestEndpointTemplate) => {
            const relativePath = joinParent(newFolderParent, folderName);
            if (useRestEndpointTemplate) {
              await createRestEndpointFolder(
                project.docsRoot!,
                relativePath,
                folderName,
              );
            } else {
              await createProjectDir(project.docsRoot!, relativePath);
            }
            session.ensureExpanded(relativePath);
            setNewFolderParent(null);
            await tree.refresh();
            if (useRestEndpointTemplate) {
              await editor.openFile(joinParent(relativePath, `${folderName}.adoc`));
            }
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
            const report = renameTarget.isDir
              ? await renameProjectDir(project.docsRoot!, oldPath, newPath)
              : await renameProjectFile(project.docsRoot!, oldPath, newPath);
            editor.remapTabsUnder(oldPath, newPath);
            session.remapExpandedUnder(oldPath, newPath);
            session.ensureExpanded(parentOfPath(newPath));
            setRenameTarget(null);
            await tree.refresh();
            git.scheduleRefresh();
            void applyRenameReport(report);
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
          branchName={project.branchName}
          hasUpstream={git.status.hasUpstream}
          ahead={git.status.ahead}
          busy={git.busy}
          onCancel={() => setPushConfirmOpen(false)}
          onConfirm={() => void onPushConfirm()}
        />
      ) : null}

      {branchSwitchBlocked ? (
        <CheckoutBlockedModal
          branchName={branchSwitchBlocked.branchName}
          mode="create"
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
          onSaveContent={handleGitSaveContent}
        />
      ) : null}

      {conflictTarget ? (
        <GitConflictModal
          path={conflictTarget}
          busy={git.busy}
          editorFontSizePx={generalPrefs.prefs.editorFontSizePx}
          onClose={() => setConflictTarget(null)}
          onLoadContent={git.loadConflictFile}
          onResolve={onResolveConflict}
        />
      ) : null}

      {commitFileDiffTarget ? (
        <GitCommitFileDiffModal
          commitHash={commitFileDiffTarget.commitHash}
          file={commitFileDiffTarget.file}
          editorFontSizePx={generalPrefs.prefs.editorFontSizePx}
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

      {gitAlert ? (
        <AlertOkModal
          title={gitAlert.title}
          message={friendlyGitError(gitAlert.message)}
          variant={gitAlert.variant}
          onClose={() => setGitAlert(null)}
        />
      ) : null}

      <DocsSearchOverlay
        open={docsSearchOpen}
        docsRoot={project.docsRoot}
        onClose={() => setDocsSearchOpen(false)}
        onOpenHit={(path, line) => {
          void openDocsSearchHit(path, line);
        }}
      />

      {activeToast ? (
        <div
          className={`app-toast ${activeToast.variant === "success" ? "app-toast-success" : ""}`}
          role="status"
        >
          <span className="app-toast-message">{activeToast.message}</span>
          <button
            type="button"
            className="app-toast-close"
            onClick={activeToast.onClose}
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
