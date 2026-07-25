import { useCallback, useEffect, useRef, useState } from "react";
import {
  clampPanelLayout,
  DEFAULT_PANEL_LAYOUT,
  getProjectLayout,
  PANEL_LAYOUT_LIMITS,
  type PanelLayout,
  saveProjectLayout,
} from "../lib/projectLayout";

const COLLAPSE_OVERSHOOT_RATIO = 0.8;

type CollapseHandlers = {
  onCollapseSidebar?: () => void;
  onCollapseRight?: () => void;
  onCollapseBottom?: () => void;
};

type OvershootKey = "sidebar" | "right" | "bottom";

type OvershootState = Record<OvershootKey, number>;

const EMPTY_OVERSHOOT: OvershootState = {
  sidebar: 0,
  right: 0,
  bottom: 0,
};

export function usePanelLayout(
  projectRoot: string | null,
  collapse: CollapseHandlers = {},
) {
  const [layout, setLayout] = useState<PanelLayout>(DEFAULT_PANEL_LAYOUT);
  const layoutRef = useRef(layout);
  const projectRootRef = useRef(projectRoot);
  projectRootRef.current = projectRoot;

  const collapseRef = useRef(collapse);
  collapseRef.current = collapse;

  const overshootRef = useRef<OvershootState>({ ...EMPTY_OVERSHOOT });
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

  const resetOvershoot = useCallback((key?: OvershootKey) => {
    if (key) {
      overshootRef.current[key] = 0;
      return;
    }
    overshootRef.current = { ...EMPTY_OVERSHOOT };
  }, []);

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

  const resizeTowardCollapse = useCallback(
    (
      key: OvershootKey,
      current: number,
      delta: number,
      min: number,
      apply: (next: number) => void,
      onCollapse?: () => void,
    ) => {
      const growing = delta > 0;
      if (growing) {
        resetOvershoot(key);
        apply(current + delta);
        return;
      }

      const next = current + delta;
      if (next > min) {
        resetOvershoot(key);
        apply(next);
        return;
      }

      // Hit or past min: keep size at min and accumulate further shrink.
      apply(min);
      const alreadyAtMin = current <= min;
      const overshootStep = alreadyAtMin ? -delta : min - next;
      overshootRef.current[key] += overshootStep;

      if (overshootRef.current[key] >= min * COLLAPSE_OVERSHOOT_RATIO) {
        resetOvershoot(key);
        onCollapse?.();
      }
    },
    [resetOvershoot],
  );

  const resizeSidebarBy = useCallback(
    (delta: number) => {
      resizeTowardCollapse(
        "sidebar",
        layoutRef.current.sidebarWidth,
        delta,
        PANEL_LAYOUT_LIMITS.sidebarWidth.min,
        (sidebarWidth) =>
          applyLayout({ ...layoutRef.current, sidebarWidth }),
        collapseRef.current.onCollapseSidebar,
      );
    },
    [applyLayout, resizeTowardCollapse],
  );

  const resizeRightBy = useCallback(
    (delta: number) => {
      resizeTowardCollapse(
        "right",
        layoutRef.current.rightWidth,
        delta,
        PANEL_LAYOUT_LIMITS.rightWidth.min,
        (rightWidth) => applyLayout({ ...layoutRef.current, rightWidth }),
        collapseRef.current.onCollapseRight,
      );
    },
    [applyLayout, resizeTowardCollapse],
  );

  const resizeBottomBy = useCallback(
    (delta: number) => {
      resizeTowardCollapse(
        "bottom",
        layoutRef.current.bottomHeight,
        delta,
        PANEL_LAYOUT_LIMITS.bottomHeight.min,
        (bottomHeight) =>
          applyLayout({ ...layoutRef.current, bottomHeight }),
        collapseRef.current.onCollapseBottom,
      );
    },
    [applyLayout, resizeTowardCollapse],
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
    resetOvershoot();
    schedulePersist(layoutRef.current);
  }, [resetOvershoot, schedulePersist]);

  return {
    layout,
    resizeSidebarBy,
    resizeRightBy,
    resizeBottomBy,
    resizeExternalBy,
    persistLayout,
  };
}
