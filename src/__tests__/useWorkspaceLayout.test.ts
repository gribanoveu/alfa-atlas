import { describe, expect, test } from "bun:test";
import { act, renderHook } from "@testing-library/react";
import { DEFAULT_PANEL_UI, useWorkspaceLayout } from "../hooks/useWorkspaceLayout";

describe("useWorkspaceLayout", () => {
  test("starts with the sidebar open and both docks closed", () => {
    const { result } = renderHook(() => useWorkspaceLayout());
    expect(result.current.sidebarOpen).toBe(DEFAULT_PANEL_UI.sidebarOpen);
    expect(result.current.activeTool).toBeNull();
    expect(result.current.bottomTool).toBeNull();
  });

  test("toggling a tool opens it, toggling the same one closes it", () => {
    const { result } = renderHook(() => useWorkspaceLayout());

    act(() => result.current.toggleRightTool("git"));
    expect(result.current.activeTool).toBe("git");

    act(() => result.current.toggleRightTool("git"));
    expect(result.current.activeTool).toBeNull();
  });

  test("toggling a different tool switches rather than closes", () => {
    const { result } = renderHook(() => useWorkspaceLayout());
    act(() => result.current.toggleRightTool("git"));
    act(() => result.current.toggleRightTool("assistant"));
    expect(result.current.activeTool).toBe("assistant");
  });

  test("the bottom dock toggles independently of the right one", () => {
    const { result } = renderHook(() => useWorkspaceLayout());
    act(() => result.current.toggleRightTool("git"));
    act(() => result.current.toggleBottomTool("problems"));

    expect(result.current.activeTool).toBe("git");
    expect(result.current.bottomTool).toBe("problems");

    act(() => result.current.toggleBottomTool("problems"));
    expect(result.current.bottomTool).toBeNull();
    expect(result.current.activeTool).toBe("git");
  });

  test("the sidebar toggles on its own", () => {
    const { result } = renderHook(() => useWorkspaceLayout());
    act(() => result.current.toggleSidebar());
    expect(result.current.sidebarOpen).toBe(false);
    act(() => result.current.toggleSidebar());
    expect(result.current.sidebarOpen).toBe(true);
  });

  test("hydrate restores a saved arrangement", () => {
    const { result } = renderHook(() => useWorkspaceLayout());
    act(() =>
      result.current.hydrate({
        sidebarOpen: false,
        rightTool: "assistant",
        bottomTool: "gitHistory",
      }),
    );

    expect(result.current.sidebarOpen).toBe(false);
    expect(result.current.activeTool).toBe("assistant");
    expect(result.current.bottomTool).toBe("gitHistory");
  });

  test("hydrate falls back to defaults for anything missing", () => {
    const { result } = renderHook(() => useWorkspaceLayout());
    act(() => result.current.toggleSidebar());

    act(() => result.current.hydrate({}));
    expect(result.current.sidebarOpen).toBe(DEFAULT_PANEL_UI.sidebarOpen);
    expect(result.current.activeTool).toBeNull();
    expect(result.current.bottomTool).toBeNull();
  });

  test("an unknown tool name from disk does not become the active tool", () => {
    // Saved state outlives the tool that wrote it: a renamed or removed
    // panel must not leave the dock pointing at something unrenderable.
    const { result } = renderHook(() => useWorkspaceLayout());
    act(() =>
      result.current.hydrate({ rightTool: "no-such-tool", bottomTool: "gone" }),
    );
    expect(result.current.activeTool).toBeNull();
    expect(result.current.bottomTool).toBeNull();
  });

  test("an explicit null tool stays closed", () => {
    const { result } = renderHook(() => useWorkspaceLayout());
    act(() => result.current.toggleRightTool("git"));
    act(() => result.current.hydrate({ rightTool: null }));
    expect(result.current.activeTool).toBeNull();
  });

  test("reset returns everything to the defaults", () => {
    const { result } = renderHook(() => useWorkspaceLayout());
    act(() => result.current.toggleSidebar());
    act(() => result.current.setRightTool("branches"));
    act(() => result.current.setBottomToolId("formatting"));

    act(() => result.current.reset());
    expect(result.current.sidebarOpen).toBe(DEFAULT_PANEL_UI.sidebarOpen);
    expect(result.current.activeTool).toBeNull();
    expect(result.current.bottomTool).toBeNull();
  });

  test("the setters address each dock directly", () => {
    const { result } = renderHook(() => useWorkspaceLayout());
    act(() => result.current.setRightTool("suggestions"));
    act(() => result.current.setSidebarOpen(false));
    act(() => result.current.setBottomToolId("problems"));

    expect(result.current.activeTool).toBe("suggestions");
    expect(result.current.sidebarOpen).toBe(false);
    expect(result.current.bottomTool).toBe("problems");
  });
});
