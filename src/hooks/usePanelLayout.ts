import { useCallback, useEffect, useRef, useState } from "react";
import {
  clampPanelLayout,
  DEFAULT_PANEL_LAYOUT,
  getProjectLayout,
  type PanelLayout,
  saveProjectLayout,
} from "../lib/projectLayout";

export function usePanelLayout(projectRoot: string | null) {
  const [layout, setLayout] = useState<PanelLayout>(DEFAULT_PANEL_LAYOUT);
  const layoutRef = useRef(layout);
  const projectRootRef = useRef(projectRoot);
  projectRootRef.current = projectRoot;

  const persistTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const applyLayout = useCallback((next: PanelLayout) => {
    const clamped = clampPanelLayout(next);
    layoutRef.current = clamped;
    setLayout(clamped);
    return clamped;
  }, []);

  const persistNow = useCallback(async (root: string, next: PanelLayout) => {
    try {
      await saveProjectLayout(root, next);
    } catch {
      // Ignore persistence failures during resize; UI keeps working.
    }
  }, []);

  const schedulePersist = useCallback((next: PanelLayout) => {
    const root = projectRootRef.current;
    if (!root) return;
    if (persistTimer.current) clearTimeout(persistTimer.current);
    persistTimer.current = setTimeout(() => {
      void persistNow(root, next);
    }, 150);
  }, [persistNow]);

  useEffect(() => {
    let cancelled = false;

    if (!projectRoot) {
      applyLayout(DEFAULT_PANEL_LAYOUT);
      return;
    }

    (async () => {
      try {
        const loaded = clampPanelLayout(await getProjectLayout(projectRoot));
        if (!cancelled) applyLayout(loaded);
      } catch {
        if (!cancelled) applyLayout(DEFAULT_PANEL_LAYOUT);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [applyLayout, projectRoot]);

  useEffect(() => {
    return () => {
      if (persistTimer.current) clearTimeout(persistTimer.current);
    };
  }, []);

  const resizeSidebarBy = useCallback(
    (delta: number) => {
      applyLayout({
        ...layoutRef.current,
        sidebarWidth: layoutRef.current.sidebarWidth + delta,
      });
    },
    [applyLayout],
  );

  const resizeRightBy = useCallback(
    (delta: number) => {
      applyLayout({
        ...layoutRef.current,
        rightWidth: layoutRef.current.rightWidth + delta,
      });
    },
    [applyLayout],
  );

  const resizeBottomBy = useCallback(
    (delta: number) => {
      applyLayout({
        ...layoutRef.current,
        bottomHeight: layoutRef.current.bottomHeight + delta,
      });
    },
    [applyLayout],
  );

  const resizeExternalBy = useCallback(
    (delta: number) => {
      applyLayout({
        ...layoutRef.current,
        externalHeight: layoutRef.current.externalHeight + delta,
      });
    },
    [applyLayout],
  );

  const persistLayout = useCallback(() => {
    schedulePersist(layoutRef.current);
  }, [schedulePersist]);

  return {
    layout,
    resizeSidebarBy,
    resizeRightBy,
    resizeBottomBy,
    resizeExternalBy,
    persistLayout,
  };
}
