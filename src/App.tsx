import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { BottomDock } from "./components/BottomDock/BottomDock";
import { EditorPane } from "./components/Editor/Editor";
import { AlertOkModal } from "./components/Git/AlertOkModal";
import { PullUpdateModal } from "./components/Git/PullUpdateModal";
import { RightDock } from "./components/RightDock/RightDock";
import { NewFileModal } from "./components/Sidebar/NewFileModal";
import { NewFolderModal } from "./components/Sidebar/NewFolderModal";
import { DeleteConfirmModal } from "./components/Sidebar/DeleteConfirmModal";
import { Sidebar } from "./components/Sidebar/Sidebar";
import { StatusBar } from "./components/StatusBar/StatusBar";
import { TopBar } from "./components/TopBar/TopBar";
import { ConfirmOpenProjectModal } from "./components/Welcome/ConfirmOpenProjectModal";
import { Welcome } from "./components/Welcome/Welcome";
import type { PullMode } from "./lib/git";
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
import { createProjectDir, createProjectFile, deleteProjectDir, deleteProjectFile, renameProjectDir, renameProjectFile } from "./lib/project";
import type { FileTreeDeleteTarget } from "./components/Sidebar/FileTree";
import { formatLabelFor, lineEndingLabelFor } from "./lib/supportedFiles";
import { RenameModal } from "./components/Sidebar/RenameModal";

function joinParent(parentPath: string, name: string): string {
  if (!parentPath || parentPath === ".") return name;
  return `${parentPath.replace(/[/\\]+$/, "")}/${name}`;
}

function parentOfPath(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  if (parts.length <= 1) return ".";
  return parts.slice(0, -1).join("/");
}

/**
 * Convert a `documentId` (a repo-relative `/`-joined index key, e.g.
 * `src/docs/asciidoc/foo.adoc`) into a path relative to `docsRoot`
 * (e.g. `foo.adoc`) — which is what `editor.openFile` expects.
 *
 * `repoRoot` and `docsRoot` are absolute filesystem paths. We compute the
 * docsRoot suffix relative to repoRoot and strip it from `documentId`.
 * If `documentId` doesn't start with that suffix, it's returned unchanged
 * (best-effort fallback).
 */
function toDocsRelativePath(
  documentId: string,
  repoRoot: string,
  docsRoot: string,
): string {
  if (!repoRoot || !docsRoot) return documentId;
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/^[/\\]+/, "").replace(/[/\\]+$/, "");
  const repo = norm(repoRoot);
  const docs = norm(docsRoot);
  // docsRoot-суффикс относительно repoRoot, например "src/docs/asciidoc".
  let suffix = docs;
  if (suffix.startsWith(repo + "/")) suffix = suffix.slice(repo.length + 1);
  const doc = norm(documentId);
  if (suffix && doc.startsWith(suffix + "/")) return doc.slice(suffix.length + 1);
  return documentId;
}

/**
 * Обратное к `toDocsRelativePath`: склеивает docs-relative путь активного
 * таба с docsRoot-суффиксом, чтобы получить repo-relative `documentId`,
 * по которому диагностики хранятся в индексе.
 */
function toRepoRelativePath(
  docsRelativePath: string,
  repoRoot: string,
  docsRoot: string,
): string {
  if (!repoRoot || !docsRoot) return docsRelativePath;
  const norm = (p: string) =>
    p.replace(/\\/g, "/").replace(/^[/\\]+/, "").replace(/[/\\]+$/, "");
  const repo = norm(repoRoot);
  const docs = norm(docsRoot);
  let suffix = docs;
  if (suffix.startsWith(repo + "/")) suffix = suffix.slice(repo.length + 1);
  const rel = norm(docsRelativePath);
  if (!suffix) return rel;
  return `${suffix}/${rel}`;
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
  const gitPanelActive =
    layout.activeTool === "git" || layout.activeTool === "gitHistory";
  const git = useGitPanel(project.repoRoot, {
    active: gitPanelActive && Boolean(project.repoRoot),
    onBranchChange: project.setBranchFromGit,
  });
  const [folderError, setFolderError] = useState<string | null>(null);
  const [newFileParent, setNewFileParent] = useState<string | null>(null);
  const [newFolderParent, setNewFolderParent] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<FileTreeDeleteTarget | null>(null);
  const [renameTarget, setRenameTarget] = useState<FileTreeDeleteTarget | null>(null);
  const [pullModalOpen, setPullModalOpen] = useState(false);
  const [gitAlert, setGitAlert] = useState<string | null>(null);
  const [revealRequest, setRevealRequest] = useState<{
    id: number;
    line: number;
    column: number;
    severity: "error" | "warning";
  } | null>(null);
  const revealCounter = useRef(0);
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
  const cursorLabel = hasProject
    ? `Ln ${editor.cursor.line}, Col ${editor.cursor.column}`
    : "Ln 1, Col 1";

  const openPullModal = useCallback(() => {
    if (!hasProject) return;
    setPullModalOpen(true);
  }, [hasProject]);

  const runPush = useCallback(async () => {
    if (!hasProject) return;
    const err = await git.push();
    if (err) setGitAlert(err);
  }, [git, hasProject]);

  const onPullConfirm = useCallback(
    async (mode: PullMode) => {
      const err = await git.pull(mode);
      setPullModalOpen(false);
      if (err) setGitAlert(err);
    },
    [git],
  );

  const onResetToRemote = useCallback(async () => {
    const err = await git.resetToRemote();
    setPullModalOpen(false);
    if (err) setGitAlert(err);
  }, [git]);

  const panelStyle = {
    ["--sidebar-width" as string]: `${panels.layout.sidebarWidth}px`,
    ["--right-width" as string]: `${panels.layout.rightWidth}px`,
    ["--bottom-height" as string]: `${panels.layout.bottomHeight}px`,
    ["--external-height" as string]: `${panels.layout.externalHeight}px`,
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
            if (ok && gitPanelActive) git.scheduleRefresh();
          });
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [editor.saveActive, git.scheduleRefresh, gitPanelActive, hasProject]);

  // After autosave clears dirty flags, refresh Git status when the panel is open.
  useEffect(() => {
    const dirtyCount = editor.tabs.filter((t) => t.dirty).length;
    if (
      gitPanelActive &&
      prevDirtyCount.current > 0 &&
      dirtyCount < prevDirtyCount.current
    ) {
      git.scheduleRefresh();
    }
    prevDirtyCount.current = dirtyCount;
  }, [editor.tabs, git.scheduleRefresh, gitPanelActive]);

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

  return (
    <div className="app" style={panelStyle}>
      <TopBar
        repoName={project.projectName ?? "—"}
        branchName={project.branchName ?? "—"}
        projectRoot={project.repoRoot}
        hasProject={hasProject}
        gitBusy={git.busy}
        onOpenFolder={openFolder}
        onCloseProject={closeProject}
        onSave={async () => {
          const ok = await editor.saveActive();
          if (ok && gitPanelActive) git.scheduleRefresh();
          return ok;
        }}
        onPrefsChange={generalPrefs.setPrefs}
        onToggleSidebar={layout.toggleSidebar}
        onToggleRight={toggleRightPanel}
        onToggleBottom={toggleBottomPanel}
        onToggleGit={toggleGitPanel}
        onPull={openPullModal}
        onPush={() => void runPush()}
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
              if (gitPanelActive) git.scheduleRefresh();
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
              onOpenProblems={openProblems}
              onOpenXref={openXref}
              viewMode={viewMode.viewMode}
              onViewModeChange={viewMode.setViewMode}
              docsRoot={project.docsRoot}
            />
          ) : (
            <Welcome
              onOpenFolder={openFolder}
              onOpenRecent={openRecent}
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
                    onStageAll: () =>
                      void git.stage(git.status.unstaged.map((f) => f.path)),
                    onUnstageAll: () =>
                      void git.unstage(git.status.staged.map((f) => f.path)),
                    onCommit: () => void git.commit(),
                    onRefresh: () => void git.refresh(),
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
            if (gitPanelActive) git.scheduleRefresh();
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
            if (gitPanelActive) git.scheduleRefresh();
          }}
        />
      ) : null}

      {pullModalOpen ? (
        <PullUpdateModal
          busy={git.busy}
          onCancel={() => setPullModalOpen(false)}
          onConfirm={(mode) => void onPullConfirm(mode)}
          onResetToRemote={() => void onResetToRemote()}
        />
      ) : null}

      {gitAlert ? (
        <AlertOkModal
          message={gitAlert}
          onClose={() => setGitAlert(null)}
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
