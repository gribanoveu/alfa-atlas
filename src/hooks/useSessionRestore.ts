import { useEffect, useRef } from "react";
import type { useDocsTree } from "./useDocsTree";
import type { useEditorTabs } from "./useEditorTabs";
import type { useProject } from "./useProject";
import { collectDirPaths, type useWorkspaceSession } from "./useWorkspaceSession";
import type { useWorkspaceLayout } from "./useWorkspaceLayout";

type Deps = {
  project: ReturnType<typeof useProject>;
  session: ReturnType<typeof useWorkspaceSession>;
  editor: ReturnType<typeof useEditorTabs>;
  tree: ReturnType<typeof useDocsTree>;
  layout: ReturnType<typeof useWorkspaceLayout>;
};

/** Puts a reopened project back the way it was left, and keeps writing the
 * current arrangement back as it changes.
 *
 * All effects, no state of its own — which is why it returns nothing. What
 * it owns are two pieces of bookkeeping that only make sense together:
 *
 * - `skipNextPanelSync` breaks the loop between hydrating the layout and
 *   persisting it. Hydration changes the very values the persist effect
 *   watches, so without this the restore would immediately write itself
 *   back, and a project would slowly overwrite its own saved layout.
 * - `seededDocsRoot` makes the first-open expansion happen once per project.
 *   Seeding depends on `tree.nodes`, which arrives asynchronously, so the
 *   effect can re-run several times before the tree settles. */
export function useSessionRestore({ project, session, editor, tree, layout }: Deps) {
  const skipNextPanelSync = useRef(false);
  const seededDocsRoot = useRef<string | null>(null);

  useEffect(() => {
    if (!session.ready || !session.loadedState || !project.docsRoot) return;
    void editor.restoreTabs(
      session.loadedState.openTabs,
      session.loadedState.activeTab,
    );
  }, [session.ready, session.loadedState, project.docsRoot, editor.restoreTabs]);

  useEffect(() => {
    if (!session.ready || !session.loadedState || !project.docsRoot) return;
    skipNextPanelSync.current = true;
    layout.hydrate(session.loadedState);
  }, [session.ready, session.loadedState, project.docsRoot, layout.hydrate]);

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

  /** A project opened for the first time gets its top-level folders expanded,
   * so the tree is not a single collapsed root. Skipped once the user has an
   * arrangement of their own — an explicitly saved set, or anything already
   * expanded in this session. */
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

  /** Tells the persist effect to skip the next layout change.
   *
   * Closing a project resets the layout, and that reset is not an
   * arrangement the user chose — writing it back would wipe the saved one
   * for the project being closed. Same flag the hydrate effect above uses,
   * exposed because the close path lives elsewhere. */
  return {
    suppressNextPanelSync: () => {
      skipNextPanelSync.current = true;
    },
  };
}
