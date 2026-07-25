import { useCallback, useEffect, useRef, useState } from "react";
import { BottomDock } from "./components/BottomDock/BottomDock";
import { EditorPane } from "./components/Editor/Editor";
import { RightDock } from "./components/RightDock/RightDock";
import { NewFileModal } from "./components/Sidebar/NewFileModal";
import { NewFolderModal } from "./components/Sidebar/NewFolderModal";
import { Sidebar } from "./components/Sidebar/Sidebar";
import { StatusBar } from "./components/StatusBar/StatusBar";
import { TopBar } from "./components/TopBar/TopBar";
import { ConfirmOpenProjectModal } from "./components/Welcome/ConfirmOpenProjectModal";
import { Welcome } from "./components/Welcome/Welcome";
import { useDocsTree } from "./hooks/useDocsTree";
import { useEditorTabs } from "./hooks/useEditorTabs";
import { useGeneralPrefs } from "./hooks/useGeneralPrefs";
import { usePanelLayout } from "./hooks/usePanelLayout";
import { useProject } from "./hooks/useProject";
import { useWorkspaceLayout } from "./hooks/useWorkspaceLayout";
import {
  collectDirPaths,
  useWorkspaceSession,
} from "./hooks/useWorkspaceSession";
import { createProjectDir, createProjectFile } from "./lib/project";
import { formatLabelFor, lineEndingLabelFor } from "./lib/supportedFiles";

function joinParent(parentPath: string, name: string): string {
  if (!parentPath || parentPath === ".") return name;
  return `${parentPath.replace(/[/\\]+$/, "")}/${name}`;
}

function App() {
  const layout = useWorkspaceLayout();
  const project = useProject();
  const generalPrefs = useGeneralPrefs();
  const panels = usePanelLayout(project.repoRoot);
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
  const [folderError, setFolderError] = useState<string | null>(null);
  const [newFileParent, setNewFileParent] = useState<string | null>(null);
  const [newFolderParent, setNewFolderParent] = useState<string | null>(null);
  const skipNextPanelSync = useRef(false);

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
  const cursorLabel = hasProject
    ? `Ln ${editor.cursor.line}, Col ${editor.cursor.column}`
    : "Ln 1, Col 1";

  const panelStyle = {
    ["--sidebar-width" as string]: `${panels.layout.sidebarWidth}px`,
    ["--right-width" as string]: `${panels.layout.rightWidth}px`,
    ["--bottom-height" as string]: `${panels.layout.bottomHeight}px`,
  };

  const openFolder = useCallback(async () => {
    setFolderError(null);
    try {
      await project.openFolderDialog();
    } catch (e) {
      setFolderError(e instanceof Error ? e.message : String(e));
    }
  }, [project]);

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

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        if (hasProject) void editor.saveActive();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [editor.saveActive, hasProject]);

  const activePath = editor.activeTab?.path ?? null;
  const statusPath = activePath ?? "—";
  const statusFormat = activePath ? formatLabelFor(activePath) : "—";
  const statusLineEnding = editor.activeTab
    ? lineEndingLabelFor(editor.activeTab.content)
    : "—";

  return (
    <div className="app" style={panelStyle}>
      <TopBar
        repoName={project.projectName ?? "—"}
        branchName={project.branchName ?? "—"}
        projectRoot={project.repoRoot}
        hasProject={hasProject}
        onOpenFolder={openFolder}
        onCloseProject={closeProject}
        onSave={editor.saveActive}
        onPrefsChange={generalPrefs.setPrefs}
        onToggleSidebar={layout.toggleSidebar}
        onToggleRight={toggleRightPanel}
        onToggleBottom={toggleBottomPanel}
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
            onToggleDir={session.toggleDir}
            onOpenFile={(path) => void editor.openFile(path)}
            onNewFile={setNewFileParent}
            onNewFolder={setNewFolderParent}
            onResize={panels.resizeSidebarBy}
            onResizeEnd={panels.persistLayout}
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
            />
          ) : (
            <Welcome
              onOpenFolder={openFolder}
              error={project.ready ? (folderError ?? project.error) : null}
            />
          )}
          <RightDock
            activeTool={layout.activeTool}
            onToggleTool={layout.toggleRightTool}
            onHide={() => layout.setRightTool(null)}
            onResize={panels.resizeRightBy}
            onResizeEnd={panels.persistLayout}
          />
        </div>
        <BottomDock
          activeTool={layout.bottomTool}
          onToggleTool={layout.toggleBottomTool}
          onHide={() => layout.setBottomToolId(null)}
          onResize={panels.resizeBottomBy}
          onResizeEnd={panels.persistLayout}
        />
      </div>
      <StatusBar
        filePath={hasProject ? statusPath : "—"}
        formatLabel={statusFormat}
        lineEndingLabel={statusLineEnding}
        cursorLabel={cursorLabel}
        hasActiveFile={Boolean(hasProject && activePath)}
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

      {editor.error || folderError ? (
        <div className="app-toast" role="status">
          {editor.error ?? folderError}
        </div>
      ) : null}
    </div>
  );
}

export default App;
