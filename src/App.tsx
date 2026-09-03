import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type * as Monaco from "monaco-editor";
import { toMessage } from "./lib/errors";
import { BottomDock } from "./components/BottomDock/BottomDock";
import { EditorPane } from "./components/Editor/Editor";
import { AlertOkModal } from "./components/Git/AlertOkModal";
import { GitConflictModal } from "./components/Git/GitConflictModal";
import { GitFileDiffModal } from "./components/Git/GitFileDiffModal";
import { GitCommitFileDiffModal } from "./components/Git/GitCommitFileDiffModal";
import { DropUnpushedConfirmModal } from "./components/Git/DropUnpushedConfirmModal";
import { MoveUnpushedModal } from "./components/Git/MoveUnpushedModal";
import { GitCommitPreviewModal } from "./components/Git/GitCommitPreviewModal";
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
import { hasTrackedGitChanges } from "./lib/git";
import { friendlyGitError } from "./lib/gitErrors";
import { useBranches } from "./hooks/useBranches";
import { useGitStash } from "./hooks/useGitStash";
import { useGitActionLog } from "./hooks/useGitActionLog";
import { useDocsTree } from "./hooks/useDocsTree";
import { useDocNavigation } from "./hooks/useDocNavigation";
import { useFileTreeActions } from "./hooks/useFileTreeActions";
import { useAppShortcuts } from "./hooks/useAppShortcuts";
import { useAssistantBridge } from "./hooks/useAssistantBridge";
import { useSessionRestore } from "./hooks/useSessionRestore";
import { useEditorTabActions } from "./hooks/useEditorTabActions";
import { useEditorTabs } from "./hooks/useEditorTabs";
import { useSpecsRepo } from "./hooks/useSpecsRepo";
import { useOpenApiBundle } from "./hooks/useOpenApiBundle";
import { OpenApiExplorer } from "./components/OpenApiExplorer/OpenApiExplorer";
import { UtilityView } from "./components/Utilities/UtilityView";
import { utilityTabId } from "./data/utilities";
import { artifactTabId, createAndOpenArtifact } from "./lib/artifactTabs";
import {
  normalizeDiagramTheme,
  resolveDiagramBackdrop,
  resolvePlantumlBackdrop,
} from "./lib/prefs";
import { setDiagramTheme, useDiagramTheme } from "./lib/diagramTheme";
import { visualTabId, type Visual } from "./lib/visuals";
import { VisualView } from "./components/Visuals/VisualView";
import { ARTIFACT_KINDS } from "./data/artifactKinds";
import { ArtifactView } from "./components/Artifacts/ArtifactView";
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
} from "./lib/project";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import {
  formatLabelFor,
  isAsciiDocPath,
  lineEndingLabelFor,
} from "./lib/supportedFiles";
import {
  isUnderDocsRoot,
  toDocsRelativePath,
  toRepoRelativePath,
} from "./lib/paths";
import { RenameModal } from "./components/Sidebar/RenameModal";
import { useOsFileDrop } from "./hooks/useOsFileDrop";

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
    activeUtility,
    activeKind,
    setActiveKind,
    displayTabs,
    openApiExplorerTab,
    openUtilityTab,
    activeArtifact,
    openArtifactTab,
    setArtifactTitle,
    setArtifactDirtyFlag,
    activeVisual,
    openVisualTab,
  } = tabs;

  // Раньше рендер спецификации открывался только одной кнопкой в сайдбаре, и
  // пользователи его не находили: естественный жест — открыть входной документ
  // и нажать «Рендер» — показывал обычное дерево ключей YAML. Считаем путь
  // входного документа в docs-relative виде (в нём живут вкладки редактора) и
  // отдаём для него в качестве превью сам API Explorer.
  const specEntryDocsPath = useMemo(() => {
    const entry = specsRepo.info?.entryFile;
    if (!entry || !project.repoRoot || !project.docsRoot) return null;
    if (!isUnderDocsRoot(entry, project.repoRoot, project.docsRoot)) return null;
    return toDocsRelativePath(entry, project.repoRoot, project.docsRoot);
  }, [specsRepo.info, project.repoRoot, project.docsRoot]);

  const activeFilePath = editor.activeTab?.path ?? null;
  const specEntryActive =
    activeKind === "file" &&
    specEntryDocsPath !== null &&
    activeFilePath === specEntryDocsPath;

  // Любой файл спецификации (входной документ или фрагмент из schemas/,
  // operations/, …) даёт в полосе вкладок кнопку «API Explorer».
  const inSpecsTree = useMemo(() => {
    if (!specsRepo.info || !activeFilePath || !project.repoRoot || !project.docsRoot) {
      return false;
    }
    const repoRelative = toRepoRelativePath(
      activeFilePath,
      project.repoRoot,
      project.docsRoot,
    );
    return repoRelative.replace(/\\/g, "/").startsWith("specs/");
  }, [specsRepo.info, activeFilePath, project.repoRoot, project.docsRoot]);

  const openApiBundle = useOpenApiBundle(
    project.repoRoot,
    specsRepo.info?.entryFile ?? null,
    // Резолв бандла стоит дорого, поэтому для файловой вкладки платим за него
    // только когда рендер действительно виден.
    openApiTabOpen || (specEntryActive && viewMode.viewMode !== "source"),
  );

  // Бандл собирается бэкендом из файлов на диске, а не из буфера редактора,
  // поэтому после сохранения спецификации (шорткат или автосохранение — оба
  // сбрасывают dirty) рендер нужно перечитать, иначе в сплите он остаётся на
  // предыдущей версии.
  const specTabDirty = inSpecsTree && (editor.activeTab?.dirty ?? false);
  const specTabWasDirty = useRef(false);
  useEffect(() => {
    if (specTabWasDirty.current && !specTabDirty && openApiBundle.bundle) {
      void openApiBundle.reload();
    }
    specTabWasDirty.current = specTabDirty;
  }, [specTabDirty, openApiBundle.bundle, openApiBundle.reload]);

  useEffect(() => {
    const onOpenPlan = (event: Event) => {
      const planId = (event as CustomEvent<{ planId?: string }>).detail?.planId;
      if (!planId) return;
      void editor.openPlan(planId);
    };
    window.addEventListener("atlas-open-plan", onOpenPlan);
    return () => window.removeEventListener("atlas-open-plan", onOpenPlan);
  }, [editor.openPlan]);

  // Dispatched by the assistant's artifact card and by the artifacts list —
  // the same cross-component escape hatch `atlas-open-plan` already uses.
  useEffect(() => {
    const onOpenArtifact = (event: Event) => {
      const artifactId = (event as CustomEvent<{ artifactId?: string }>).detail?.artifactId;
      if (!artifactId) return;
      openArtifactTab(artifactId);
    };
    window.addEventListener("atlas-open-artifact", onOpenArtifact);
    return () => window.removeEventListener("atlas-open-artifact", onOpenArtifact);
  }, [openArtifactTab]);

  // Dispatched by the assistant's `visualize` card. Unlike the two above,
  // the event carries the whole payload rather than an id: a visualization
  // has no store to load it back from — it lives on the chat message that
  // produced it (see `src/lib/visuals.ts`).
  useEffect(() => {
    const onOpenVisual = (event: Event) => {
      const visual = (event as CustomEvent<Visual>).detail;
      if (!visual?.id) return;
      openVisualTab(visual);
    };
    window.addEventListener("atlas-open-visual", onOpenVisual);
    return () => window.removeEventListener("atlas-open-visual", onOpenVisual);
  }, [openVisualTab]);








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
  // Открытие шторки веток равнозначно нажатию «Обновить список локально»:
  // сам по себе список читается один раз при открытии проекта, а к моменту,
  // когда пользователь до него добирается, и состав веток, и их отставание
  // от сервера (стрелка «доступно обновление») обычно уже устарели.
  useEffect(() => {
    if (layout.activeTool !== "branches") return;
    void branches.refresh();
  }, [layout.activeTool, branches.refresh]);
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

  const { suppressNextPanelSync } = useSessionRestore({
    project,
    session,
    editor,
    tree,
    layout,
  });
  const assistant = useAssistantBridge({
    project,
    editor,
    tree,
    session,
    git,
    layout,
    openApiBundle,
    applyRenameReport,
  });
  const { insertRequest, chatInsertRequest, assistantSendRequest, assistantDraftRequest } = assistant;

  const [docsSearchOpen, setDocsSearchOpen] = useState(false);





  const mainClassName = [
    "main",
    layout.sidebarOpen ? "" : "sidebar-collapsed",
    layout.activeTool ? "" : "right-collapsed",
  ]
    .filter(Boolean)
    .join(" ");

  const hasProject = Boolean(project.docsRoot && project.repoRoot);
  useAppShortcuts({
    hasProject,
    editor,
    git,
    openDocsSearch: () => setDocsSearchOpen(true),
  });


  const {
    commitFileDiffTarget,
    commitPreviewTarget,
    conflictTarget,
    deleteBranchTarget,
    dropAllUnpushedOpen,
    dropUnpushedTarget,
    gitAlert,
    gitDiffTarget,
    handleCheckoutBranch,
    handleCommit,
    handleCreateBranch,
    handleDropAllUnpushedConfirm,
    handleDropUnpushedConfirm,
    handleGitDiscard,
    handleGitSaveContent,
    handleMoveUnpushedConfirm,
    handleStage,
    handleSyncPillClick,
    handleUndoAction,
    handleUnstage,
    loadCommitFileDiff,
    loadCommitFiles,
    loadStashFiles,
    moveUnpushedCommits,
    moveUnpushedOpen,
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
    openCommitPreview,
    openConflict,
    openGitFileDiff,
    openMoveUnpushedModal,
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
    requestDropUnpushed,
    resetRemoteConfirmOpen,
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
    syncPillState,
    unpushedBusy,
    unpushedHashSet,
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
  const [credentialsSettingsSignal, setCredentialsSettingsSignal] = useState(0);
  const [jiraSettingsSignal, setJiraSettingsSignal] = useState(0);





  const cursorLabel = hasProject
    ? `Ln ${editor.cursor.line}, Col ${editor.cursor.column}`
    : "Ln 1, Col 1";









  // `AscMermaid` reads the palette from a store rather than a prop — it
  // renders from three unrelated subtrees; see `lib/diagramTheme.ts`.
  const diagramTheme = normalizeDiagramTheme(generalPrefs.prefs.diagramTheme);
  useEffect(() => {
    setDiagramTheme(diagramTheme);
  }, [diagramTheme]);
  // Читаем палитру обратно из стора, а не из `prefs`: кнопка на панели
  // диаграммы пишет в стор напрямую (`chooseDiagramTheme`), и подложка
  // «Авто» должна переворачиваться вместе с ней, а не ждать перезагрузки
  // настроек.
  const activeDiagramTheme = useDiagramTheme();

  const panelStyle = {
    ["--sidebar-width" as string]: `${panels.layout.sidebarWidth}px`,
    ["--right-width" as string]: `${panels.layout.rightWidth}px`,
    ["--bottom-height" as string]: `${panels.layout.bottomHeight}px`,
    ["--external-height" as string]: `${panels.layout.externalHeight}px`,
    ["--font-ui" as string]: `${generalPrefs.prefs.uiFontSizePx}px`,
    ["--font-sidebar" as string]: `${generalPrefs.prefs.sidebarFontSizePx}px`,
    ["--font-editor" as string]: `${generalPrefs.prefs.editorFontSizePx}px`,
    ["--font-preview" as string]: `${generalPrefs.prefs.previewFontSizePx}px`,
    ["--font-assistant" as string]: `${generalPrefs.prefs.assistantFontSizePx}px`,
    // Resolved (so `"auto"` becomes a real colour) and normalized on the way
    // in as well as on the way out of the store: this interpolates into a
    // CSS custom property, which React does not escape.
    ["--diagram-backdrop" as string]: resolveDiagramBackdrop(
      generalPrefs.prefs.diagramBackdrop,
      activeDiagramTheme,
    ),
    // PlantUML не перекрашивается под тёмную палитру, поэтому «Авто» у него
    // всегда светлое — иначе чёрные линии оказываются на тёмной подложке.
    ["--diagram-backdrop-plantuml" as string]: resolvePlantumlBackdrop(
      generalPrefs.prefs.diagramBackdrop,
    ),
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
    suppressNextPanelSync();
    layout.reset();
  }, [editor.reset, layout.reset, project.closeProject, suppressNextPanelSync]);

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
        onCut={tabs.cut}
        onCopy={tabs.copy}
        onPaste={tabs.paste}
        getEditAvailability={tabs.editAvailability}
        hasActiveTab={editor.activeTab !== null}
        onPrefsChange={generalPrefs.setPrefs}
        onSpellcheckConfigChange={spellcheck.setConfig}
        onToggleSidebar={layout.toggleSidebar}
        onToggleRight={toggleRightPanel}
        onToggleBottom={toggleBottomPanel}
        onToggleGit={toggleGitPanel}
        onOpenBranches={() => layout.setRightTool(layout.activeTool === "branches" ? null : "branches")}
        onPull={openPullModal}
        onPush={() => openPushModal()}
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
        openCredentialsSettingsSignal={credentialsSettingsSignal}
        openJiraSettingsSignal={jiraSettingsSignal}
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
              activeTabId={
                activeKind === "openapi"
                  ? "openapi"
                  : activeKind === "utility" && activeUtility
                    ? utilityTabId(activeUtility)
                    : activeKind === "artifact" && activeArtifact
                      ? artifactTabId(activeArtifact)
                      : activeKind === "visual" && activeVisual
                        ? visualTabId(activeVisual.id)
                        : editor.activeTabId
              }
              activeTab={editor.activeTab}
              activeKind={activeKind}
              openApiExplorer={
                openApiTabOpen || specEntryActive ? (
                  <OpenApiExplorer
                    bundle={openApiBundle.bundle}
                    loading={openApiBundle.loading}
                    error={openApiBundle.error}
                  />
                ) : undefined
              }
              specEntryPath={specEntryDocsPath}
              inSpecsTree={inSpecsTree}
              onOpenApiExplorer={openApiExplorerTab}
              utilityView={
                activeUtility ? <UtilityView utilityId={activeUtility} /> : undefined
              }
              artifactView={
                activeArtifact ? (
                  <ArtifactView
                    key={activeArtifact}
                    artifactId={activeArtifact}
                    onDirtyChange={setArtifactDirtyFlag}
                    onTitleChange={setArtifactTitle}
                    onSendToAssistant={(record) => {
                      // Nothing was paused on this artifact (it was opened
                      // from the utilities panel, or its turn ended long
                      // ago) — drop the announcement into the composer
                      // rather than sending it: the user should see (and
                      // can still edit) what's about to go to the model,
                      // not have it fire the instant they click Отправить.
                      assistant.insertAssistantDraft(
                        `Я заполнил артефакт \`${record.id}\` — «${record.title}». Прочитай его тулом artifact (op: "read") и используй в документации.`,
                      );
                    }}
                  />
                ) : undefined
              }
              visualView={
                activeVisual ? <VisualView key={activeVisual.id} visual={activeVisual} /> : undefined
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
              onAddToChat={assistant.addSelectionToChat}
              onRunContextAction={(prompt, opts) => {
                if (opts?.delivery === "draft") {
                  assistant.insertAssistantDraft(prompt, opts);
                } else {
                  assistant.sendAssistantPrompt(prompt, opts);
                }
              }}
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
              onOpenSettings={() => setCredentialsSettingsSignal((n) => n + 1)}
              onOpenGitKeySettings={() => setCredentialsSettingsSignal((n) => n + 1)}
              onOpenLlmKeySettings={() => setLlmSettingsSignal((n) => n + 1)}
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
                    onInsert: assistant.insertSnippet,
                  }
                : null
            }
            utilities={
              // Вкладки живут только внутри EditorPane, а он монтируется
              // вместе с проектом — без проекта карточке некуда открываться.
              hasProject
                ? {
                    onOpen: openUtilityTab,
                    activeId: activeKind === "utility" ? activeUtility : null,
                    onNewArtifact: (kind) => {
                      const label = ARTIFACT_KINDS.find((k) => k.id === kind)?.newLabel ?? "Новый артефакт";
                      void createAndOpenArtifact(kind, label).catch((e) =>
                        setFolderError(toMessage(e)),
                      );
                    },
                    onOpenArtifacts: () => {
                      window.dispatchEvent(new CustomEvent("atlas-open-artifacts"));
                    },
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
            jira={{
              onOpenSettings: () => setJiraSettingsSignal((n) => n + 1),
            }}
            assistant={{
              onOpenSettings: () => setLlmSettingsSignal((n) => n + 1),
              specsRepoInfo: specsRepo.info,
              docsRoot: project.docsRoot ?? "",
              onFileWritten: assistant.onFileWritten,
              onFileMoved: assistant.onFileMoved,
              repoRoot: project.repoRoot,
              activeFilePath: editor.activeTab?.path ?? null,
              hasUncommittedChanges: hasTrackedGitChanges(git.status),
            }}
            chatInsertRequest={chatInsertRequest}
            onChatInsertHandled={assistant.onChatInsertHandled}
            assistantSendRequest={assistantSendRequest}
            onAssistantSendHandled={assistant.onAssistantSendHandled}
            assistantDraftRequest={assistantDraftRequest}
            onAssistantDraftHandled={assistant.onAssistantDraftHandled}
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
                  unpushedHashes: unpushedHashSet,
                  unpushedCount: git.unpushedCommits.length,
                  hasUpstream: git.status.hasUpstream,
                  dropBusy: unpushedBusy,
                  onRefresh: () => void git.refresh(),
                  onLoadCommitFiles: loadCommitFiles,
                  onOpenCommitFileDiff: openCommitFileDiff,
                  onDropCommit: (hash) =>
                    requestDropUnpushed(hash, git.unpushedCommits),
                  onMoveToBranch: () =>
                    openMoveUnpushedModal(git.unpushedCommits),
                  onDropAllUnpushed: () => setDropAllUnpushedOpen(true),
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
          branchName={project.branchName}
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
          currentBranch={project.branchName}
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
