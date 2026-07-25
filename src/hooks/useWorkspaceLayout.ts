import { useCallback, useState } from "react";

export type RightTool = "assistant" | "asciidoc" | "git";
export type BottomTool = "suggestions" | "formatting";

export type { CursorPosition, EditorTab } from "./useEditorTabs";

export function useWorkspaceLayout() {
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [activeTool, setActiveTool] = useState<RightTool | null>("assistant");
  const [bottomTool, setBottomTool] = useState<BottomTool | null>(null);

  const toggleSidebar = useCallback(() => {
    setSidebarOpen((open) => !open);
  }, []);

  const setRightTool = useCallback((tool: RightTool | null) => {
    setActiveTool(tool);
  }, []);

  const toggleRightTool = useCallback((tool: RightTool) => {
    setActiveTool((current) => (current === tool ? null : tool));
  }, []);

  const setBottomToolId = useCallback((tool: BottomTool | null) => {
    setBottomTool(tool);
  }, []);

  const toggleBottomTool = useCallback((tool: BottomTool) => {
    setBottomTool((current) => (current === tool ? null : tool));
  }, []);

  return {
    sidebarOpen,
    toggleSidebar,
    activeTool,
    setRightTool,
    toggleRightTool,
    bottomTool,
    setBottomToolId,
    toggleBottomTool,
  };
}
