import { useCallback, useEffect, useRef, useState } from "react";
import {
  DEFAULT_WORKSPACE_STATE,
  getWorkspaceState,
  saveWorkspaceState,
  type WorkspaceState,
} from "../lib/workspace";

export type PanelUiPersist = {
  sidebarOpen: boolean;
  rightTool: string | null;
  bottomTool: string | null;
};

function ancestorsOf(path: string): string[] {
  if (!path || path === ".") return ["."];
  const parts = path.split(/[/\\]/).filter(Boolean);
  const result = ["."];
  for (let i = 0; i < parts.length; i++) {
    result.push(parts.slice(0, i + 1).join("/"));
  }
  return result;
}

function defaultExpandedForDepth(maxDepth: number): string[] {
  // Only root until tree is known; FileTree may expand shallow defaults via session seed.
  void maxDepth;
  return ["."];
}

export function useWorkspaceSession(
  repoRoot: string | null,
  docsRoot: string | null,
) {
  const [expandedDirs, setExpandedDirs] = useState<Set<string>>(
    () => new Set(DEFAULT_WORKSPACE_STATE.expandedDirs),
  );
  const [ready, setReady] = useState(false);
  const [loadedState, setLoadedState] = useState<WorkspaceState | null>(null);
  const persistTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const expandedRef = useRef(expandedDirs);
  expandedRef.current = expandedDirs;
  const openTabsRef = useRef<string[]>([]);
  const activeTabRef = useRef<string | null>(null);
  const sidebarOpenRef = useRef(DEFAULT_WORKSPACE_STATE.sidebarOpen);
  const rightToolRef = useRef<string | null>(DEFAULT_WORKSPACE_STATE.rightTool);
  const bottomToolRef = useRef<string | null>(
    DEFAULT_WORKSPACE_STATE.bottomTool,
  );
  const repoRootRef = useRef(repoRoot);
  repoRootRef.current = repoRoot;

  const persistNow = useCallback(async () => {
    const root = repoRootRef.current;
    if (!root) return;
    const state: WorkspaceState = {
      openTabs: openTabsRef.current,
      activeTab: activeTabRef.current,
      expandedDirs: Array.from(expandedRef.current),
      sidebarOpen: sidebarOpenRef.current,
      rightTool: rightToolRef.current,
      bottomTool: bottomToolRef.current,
    };
    try {
      await saveWorkspaceState(root, state);
    } catch {
      // Ignore persistence failures; UI keeps working.
    }
  }, []);

  const schedulePersist = useCallback(() => {
    if (!repoRootRef.current) return;
    if (persistTimer.current) clearTimeout(persistTimer.current);
    persistTimer.current = setTimeout(() => {
      void persistNow();
    }, 200);
  }, [persistNow]);

  useEffect(() => {
    let cancelled = false;
    setReady(false);
    setLoadedState(null);

    if (!repoRoot || !docsRoot) {
      setExpandedDirs(new Set(defaultExpandedForDepth(3)));
      openTabsRef.current = [];
      activeTabRef.current = null;
      sidebarOpenRef.current = DEFAULT_WORKSPACE_STATE.sidebarOpen;
      rightToolRef.current = DEFAULT_WORKSPACE_STATE.rightTool;
      bottomToolRef.current = DEFAULT_WORKSPACE_STATE.bottomTool;
      setReady(true);
      return;
    }

    (async () => {
      try {
        const state = await getWorkspaceState(repoRoot);
        if (cancelled) return;
        const expanded = state.expandedDirs.length
          ? state.expandedDirs
          : defaultExpandedForDepth(3);
        if (!expanded.includes(".")) expanded.unshift(".");
        setExpandedDirs(new Set(expanded));
        openTabsRef.current = state.openTabs;
        activeTabRef.current = state.activeTab;
        sidebarOpenRef.current =
          state.sidebarOpen ?? DEFAULT_WORKSPACE_STATE.sidebarOpen;
        rightToolRef.current =
          state.rightTool === undefined
            ? DEFAULT_WORKSPACE_STATE.rightTool
            : state.rightTool;
        bottomToolRef.current =
          state.bottomTool === undefined
            ? DEFAULT_WORKSPACE_STATE.bottomTool
            : state.bottomTool;
        setLoadedState({
          ...DEFAULT_WORKSPACE_STATE,
          ...state,
          expandedDirs: expanded,
          sidebarOpen: sidebarOpenRef.current,
          rightTool: rightToolRef.current,
          bottomTool: bottomToolRef.current,
        });
      } catch {
        if (cancelled) return;
        setExpandedDirs(new Set(defaultExpandedForDepth(3)));
        openTabsRef.current = [];
        activeTabRef.current = null;
        sidebarOpenRef.current = DEFAULT_WORKSPACE_STATE.sidebarOpen;
        rightToolRef.current = DEFAULT_WORKSPACE_STATE.rightTool;
        bottomToolRef.current = DEFAULT_WORKSPACE_STATE.bottomTool;
        setLoadedState({ ...DEFAULT_WORKSPACE_STATE });
      } finally {
        if (!cancelled) setReady(true);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [repoRoot, docsRoot]);

  useEffect(() => {
    return () => {
      if (persistTimer.current) clearTimeout(persistTimer.current);
    };
  }, []);

  const toggleDir = useCallback(
    (path: string) => {
      setExpandedDirs((prev) => {
        const next = new Set(prev);
        if (next.has(path)) next.delete(path);
        else next.add(path);
        if (!next.has(".")) next.add(".");
        expandedRef.current = next;
        schedulePersist();
        return next;
      });
    },
    [schedulePersist],
  );

  const ensureExpanded = useCallback(
    (path: string) => {
      const ancestors = ancestorsOf(path);
      setExpandedDirs((prev) => {
        const next = new Set(prev);
        for (const p of ancestors) next.add(p);
        expandedRef.current = next;
        schedulePersist();
        return next;
      });
    },
    [schedulePersist],
  );

  const syncTabs = useCallback(
    (openTabs: string[], activeTab: string | null) => {
      openTabsRef.current = openTabs;
      activeTabRef.current = activeTab;
      schedulePersist();
    },
    [schedulePersist],
  );

  const syncPanelUi = useCallback(
    (panel: PanelUiPersist) => {
      sidebarOpenRef.current = panel.sidebarOpen;
      rightToolRef.current = panel.rightTool;
      bottomToolRef.current = panel.bottomTool;
      schedulePersist();
    },
    [schedulePersist],
  );

  const seedShallowExpanded = useCallback(
    (dirPaths: string[]) => {
      // Only seed when workspace had no custom expand beyond root.
      setExpandedDirs((prev) => {
        if (prev.size > 1) return prev;
        const next = new Set<string>(["."]);
        for (const p of dirPaths) {
          const depth = p === "." ? 0 : p.split(/[/\\]/).filter(Boolean).length;
          if (depth > 0 && depth < 3) next.add(p);
        }
        expandedRef.current = next;
        schedulePersist();
        return next;
      });
    },
    [schedulePersist],
  );

  const expandAll = useCallback(
    (dirPaths: string[]) => {
      const next = new Set<string>([".", ...dirPaths]);
      expandedRef.current = next;
      schedulePersist();
      setExpandedDirs(next);
    },
    [schedulePersist],
  );

  const collapseAll = useCallback(() => {
    const next = new Set<string>(["."]);
    expandedRef.current = next;
    schedulePersist();
    setExpandedDirs(next);
  }, [schedulePersist]);

  const remapExpandedUnder = useCallback(
    (oldPath: string, newPath: string) => {
      const prefix = oldPath.replace(/[/\\]+$/, "") + "/";
      const remap = (p: string): string => {
        if (p === oldPath) return newPath;
        if (p.startsWith(prefix)) return newPath + p.slice(prefix.length);
        if (p.startsWith(oldPath + "\\")) {
          return newPath + p.slice(oldPath.length);
        }
        return p;
      };
      setExpandedDirs((prev) => {
        let changed = false;
        const next = new Set<string>();
        for (const p of prev) {
          const r = remap(p);
          if (r !== p) changed = true;
          next.add(r);
        }
        if (!changed) return prev;
        expandedRef.current = next;
        schedulePersist();
        return next;
      });
    },
    [schedulePersist],
  );

  return {
    ready,
    loadedState,
    expandedDirs,
    toggleDir,
    ensureExpanded,
    expandAll,
    collapseAll,
    remapExpandedUnder,
    syncTabs,
    syncPanelUi,
    seedShallowExpanded,
  };
}

/** Collect directory paths from a tree (relative paths). */
export function collectDirPaths(
  nodes: { path: string; isDir: boolean; children?: unknown[] }[],
  out: string[] = [],
): string[] {
  for (const node of nodes) {
    if (!node.isDir) continue;
    out.push(node.path);
    if (node.children) {
      collectDirPaths(
        node.children as {
          path: string;
          isDir: boolean;
          children?: unknown[];
        }[],
        out,
      );
    }
  }
  return out;
}
