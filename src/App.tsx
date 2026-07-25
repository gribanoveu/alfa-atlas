import { useCallback, useEffect, useState } from "react";
import { BottomDock } from "./components/BottomDock/BottomDock";
import { EditorPane } from "./components/Editor/Editor";
import { RightDock } from "./components/RightDock/RightDock";
import { Sidebar } from "./components/Sidebar/Sidebar";
import { StatusBar } from "./components/StatusBar/StatusBar";
import { TopBar } from "./components/TopBar/TopBar";
import { ConfirmOpenProjectModal } from "./components/Welcome/ConfirmOpenProjectModal";
import { Welcome } from "./components/Welcome/Welcome";
import { useDocsTree } from "./hooks/useDocsTree";
import { useEditorTabs } from "./hooks/useEditorTabs";
import { usePanelLayout } from "./hooks/usePanelLayout";
import { useProject } from "./hooks/useProject";
import { useWorkspaceLayout } from "./hooks/useWorkspaceLayout";

function App() {
  const layout = useWorkspaceLayout();
  const project = useProject();
  const panels = usePanelLayout(project.repoRoot);
  const tree = useDocsTree(project.docsRoot);
  const editor = useEditorTabs(project.docsRoot);
  const [folderError, setFolderError] = useState<string | null>(null);

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
    await project.closeProject();
    editor.reset();
  }, [editor.reset, project.closeProject]);

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

  const statusPath = editor.activeTab?.path ?? "—";
  const statusLanguage = editor.activeTab?.language ?? "—";

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
            onOpenFile={(path) => void editor.openFile(path)}
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
        language={hasProject ? statusLanguage : "—"}
        cursorLabel={cursorLabel}
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

      {editor.error || folderError ? (
        <div className="app-toast" role="status">
          {editor.error ?? folderError}
        </div>
      ) : null}
    </div>
  );
}

export default App;
