import { BottomDock } from "./components/BottomDock/BottomDock";
import { EditorPane } from "./components/Editor/Editor";
import { RightDock } from "./components/RightDock/RightDock";
import { Sidebar } from "./components/Sidebar/Sidebar";
import { StatusBar } from "./components/StatusBar/StatusBar";
import { TopBar } from "./components/TopBar/TopBar";
import { Welcome } from "./components/Welcome/Welcome";
import { usePanelLayout } from "./hooks/usePanelLayout";
import { useProject } from "./hooks/useProject";
import { useWorkspaceLayout } from "./hooks/useWorkspaceLayout";

function App() {
  const layout = useWorkspaceLayout();
  const project = useProject();
  const panels = usePanelLayout(project.projectRoot);

  const mainClassName = [
    "main",
    layout.sidebarOpen ? "" : "sidebar-collapsed",
    layout.activeTool ? "" : "right-collapsed",
  ]
    .filter(Boolean)
    .join(" ");

  const hasProject = Boolean(project.projectRoot);
  const cursorLabel = hasProject
    ? `Ln ${layout.cursor.line}, Col ${layout.cursor.column}`
    : "Ln 1, Col 1";

  const panelStyle = {
    ["--sidebar-width" as string]: `${panels.layout.sidebarWidth}px`,
    ["--right-width" as string]: `${panels.layout.rightWidth}px`,
    ["--bottom-height" as string]: `${panels.layout.bottomHeight}px`,
  };

  return (
    <div className="app" style={panelStyle}>
      <TopBar
        repoName={project.projectName ?? "—"}
        branchName="—"
      />
      <div className="workspace">
        <div className={mainClassName}>
          <Sidebar
            open={layout.sidebarOpen}
            onToggle={layout.toggleSidebar}
            projectRoot={project.projectRoot}
            projectName={project.projectName}
            onResize={panels.resizeSidebarBy}
            onResizeEnd={panels.persistLayout}
          />
          {hasProject ? (
            <EditorPane
              tabs={layout.tabs}
              activeTabId={layout.activeTabId}
              activeTab={layout.activeTab}
              onSelectTab={layout.selectTab}
              onCloseTab={layout.closeTab}
              onChangeContent={layout.updateActiveContent}
              onCursorChange={layout.setCursor}
            />
          ) : (
            <Welcome
              onOpenFolder={project.openFolderDialog}
              error={project.ready ? project.error : null}
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
        filePath={hasProject ? layout.activeTab.title : "—"}
        language={hasProject ? layout.activeTab.language : "—"}
        cursorLabel={cursorLabel}
      />
    </div>
  );
}

export default App;
