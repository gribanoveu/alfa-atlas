import { beforeEach, describe, expect, mock, test } from "bun:test";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as actualLayout from "../lib/projectLayout";
import {
  clampPanelLayout,
  DEFAULT_PANEL_LAYOUT,
  PANEL_LAYOUT_LIMITS,
  type PanelLayout,
} from "../lib/projectLayout";

let storedLayout: PanelLayout = { ...DEFAULT_PANEL_LAYOUT };
let loadThrows: string | null = null;
let saves: Array<[string, PanelLayout]> = [];

mock.module("../lib/projectLayout", () => ({
  ...actualLayout,
  getProjectLayout: async () => {
    if (loadThrows) throw loadThrows;
    return storedLayout;
  },
  saveProjectLayout: async (root: string, layout: PanelLayout) => {
    saves.push([root, layout]);
  },
}));

const { usePanelLayout } = await import("../hooks/usePanelLayout");

const SIDEBAR_MIN = PANEL_LAYOUT_LIMITS.sidebarWidth.min;
const SIDEBAR_MAX = PANEL_LAYOUT_LIMITS.sidebarWidth.max;

/** Writes are debounced by 150ms. */
async function flushPersist() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 200));
  });
}

function render(root: string | null = "/repo", collapse: Record<string, () => void> = {}) {
  return renderHook(() => usePanelLayout(root, collapse));
}

beforeEach(() => {
  storedLayout = { ...DEFAULT_PANEL_LAYOUT };
  loadThrows = null;
  saves = [];
});

describe("usePanelLayout — loading", () => {
  test("loads the saved sizes for a project", async () => {
    storedLayout = { ...DEFAULT_PANEL_LAYOUT, sidebarWidth: 300 };
    const { result } = render();
    await waitFor(() => expect(result.current.layout.sidebarWidth).toBe(300));
  });

  test("saved sizes outside the allowed range are clamped, not trusted", async () => {
    // A layout file written by an older build (or edited by hand) must not
    // be able to squeeze a panel to nothing.
    storedLayout = { ...DEFAULT_PANEL_LAYOUT, sidebarWidth: 10, rightWidth: 9999 };
    const { result } = render();

    await waitFor(() => expect(result.current.layout.sidebarWidth).toBe(SIDEBAR_MIN));
    expect(result.current.layout.rightWidth).toBe(PANEL_LAYOUT_LIMITS.rightWidth.max);
  });

  test("a failing load falls back to the defaults", async () => {
    loadThrows = "layout file corrupt";
    const { result } = render();
    await waitFor(() =>
      expect(result.current.layout).toEqual(clampPanelLayout(DEFAULT_PANEL_LAYOUT)),
    );
  });

  test("no project means the defaults, clamped", async () => {
    // Note: `DEFAULT_PANEL_LAYOUT.rightWidth` (340) is below its own
    // declared minimum (400), so the effective default is 400. The constant
    // and the limit disagree; the clamp decides.
    const { result } = render(null);
    expect(result.current.layout).toEqual(clampPanelLayout(DEFAULT_PANEL_LAYOUT));
    expect(result.current.layout.rightWidth).toBe(PANEL_LAYOUT_LIMITS.rightWidth.min);
  });
});

describe("usePanelLayout — resizing", () => {
  test("dragging wider grows the panel", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.layout.sidebarWidth).toBe(220));

    act(() => result.current.resizeSidebarBy(60));
    expect(result.current.layout.sidebarWidth).toBe(280);
  });

  test("growth stops at the maximum", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.layout.sidebarWidth).toBe(220));

    act(() => result.current.resizeSidebarBy(10_000));
    expect(result.current.layout.sidebarWidth).toBe(SIDEBAR_MAX);
  });

  test("shrinking stops at the minimum rather than going further", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.layout.sidebarWidth).toBe(220));

    act(() => result.current.resizeSidebarBy(-1000));
    expect(result.current.layout.sidebarWidth).toBe(SIDEBAR_MIN);
  });

  test("the panels resize independently", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.layout.sidebarWidth).toBe(220));

    act(() => result.current.resizeBottomBy(40));
    expect(result.current.layout.bottomHeight).toBe(260);
    expect(result.current.layout.sidebarWidth).toBe(220);
  });
});

describe("usePanelLayout — drag past the edge collapses", () => {
  /** A real drag arrives as many small deltas, not one big jump. */
  function drag(step: () => void, times: number) {
    for (let i = 0; i < times; i++) act(step);
  }

  test("continuing to drag once the panel is at its minimum collapses it", async () => {
    // The panel cannot get smaller, so continued dragging has to mean
    // something — it closes, the way a window manager behaves.
    const onCollapseSidebar = mock(() => {});
    const { result } = render("/repo", { onCollapseSidebar });
    await waitFor(() => expect(result.current.layout.sidebarWidth).toBe(220));

    drag(() => result.current.resizeSidebarBy(-20), 3);
    expect(result.current.layout.sidebarWidth).toBe(SIDEBAR_MIN);
    expect(onCollapseSidebar).not.toHaveBeenCalled();

    // 80% of the minimum has to accumulate past the edge before it closes.
    drag(() => result.current.resizeSidebarBy(-20), 7);
    expect(onCollapseSidebar).toHaveBeenCalled();
  });

  test("a nudge past the minimum is not enough", async () => {
    const onCollapseSidebar = mock(() => {});
    const { result } = render("/repo", { onCollapseSidebar });
    await waitFor(() => expect(result.current.layout.sidebarWidth).toBe(220));

    drag(() => result.current.resizeSidebarBy(-20), 4);
    expect(result.current.layout.sidebarWidth).toBe(SIDEBAR_MIN);
    expect(onCollapseSidebar).not.toHaveBeenCalled();
  });

  test("dragging back out cancels the accumulated overshoot", async () => {
    // Otherwise a hesitant drag — in, out, in — would close a panel the
    // user was trying to keep.
    const onCollapseSidebar = mock(() => {});
    const { result } = render("/repo", { onCollapseSidebar });
    await waitFor(() => expect(result.current.layout.sidebarWidth).toBe(220));

    drag(() => result.current.resizeSidebarBy(-20), 7);
    act(() => result.current.resizeSidebarBy(20));
    drag(() => result.current.resizeSidebarBy(-20), 5);

    expect(onCollapseSidebar).not.toHaveBeenCalled();
  });

  test("one large jump past the edge collapses immediately", async () => {
    // Overshoot is measured by distance past the minimum, not by the number
    // of drag events — a fast flick counts as much as a slow push.
    const onCollapseSidebar = mock(() => {});
    const { result } = render("/repo", { onCollapseSidebar });
    await waitFor(() => expect(result.current.layout.sidebarWidth).toBe(220));

    act(() => result.current.resizeSidebarBy(-1000));
    expect(onCollapseSidebar).toHaveBeenCalled();
  });

  test("the external panel has no collapse behaviour", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.layout.externalHeight).toBe(160));

    act(() => result.current.resizeExternalBy(-1000));
    expect(result.current.layout.externalHeight).toBe(
      PANEL_LAYOUT_LIMITS.externalHeight.min,
    );
  });
});

describe("usePanelLayout — persistence", () => {
  test("sizes are written once the drag ends, not during it", async () => {
    const { result } = render();
    await waitFor(() => expect(result.current.layout.sidebarWidth).toBe(220));

    act(() => result.current.resizeSidebarBy(30));
    act(() => result.current.resizeSidebarBy(30));
    await flushPersist();
    expect(saves).toEqual([]);

    act(() => result.current.persistLayout());
    await flushPersist();
    expect(saves).toHaveLength(1);
    expect(saves[0]?.[1].sidebarWidth).toBe(280);
  });

  test("nothing is written without a project", async () => {
    const { result } = render(null);
    act(() => result.current.resizeSidebarBy(30));
    act(() => result.current.persistLayout());
    await flushPersist();
    expect(saves).toEqual([]);
  });
});
