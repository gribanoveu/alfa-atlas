import { useCallback, useState } from "react";

export type RightTool =
  | "assistant"
  | "asciidoc"
  | "utilities"
  | "git"
  | "suggestions"
  | "branches"
  | "jira";
export type BottomTool = "gitHistory" | "formatting" | "problems";

export type PanelUiState = {
  sidebarOpen: boolean;
  rightTool: RightTool | null;
  bottomTool: BottomTool | null;
};

export type { CursorPosition, EditorTab } from "./useEditorTabs";

const RIGHT_TOOLS: readonly RightTool[] = [
  "assistant",
  "branches",
  "git",
  "suggestions",
  "asciidoc",
  "utilities",
  "jira",
];
const BOTTOM_TOOLS: readonly BottomTool[] = [
  "gitHistory",
  "formatting",
  "problems",
];

export const DEFAULT_PANEL_UI: PanelUiState = {
  sidebarOpen: true,
  rightTool: null,
  bottomTool: null,
};

function parseRightTool(value: string | null | undefined): RightTool | null {
  if (value === null || value === undefined) return null;
  return RIGHT_TOOLS.includes(value as RightTool)
    ? (value as RightTool)
    : DEFAULT_PANEL_UI.rightTool;
}

function parseBottomTool(value: string | null | undefined): BottomTool | null {
  if (value === null || value === undefined) return null;
  return BOTTOM_TOOLS.includes(value as BottomTool)
    ? (value as BottomTool)
    : null;
}

export function useWorkspaceLayout() {
  const [sidebarOpen, setSidebarOpen] = useState(DEFAULT_PANEL_UI.sidebarOpen);
  const [activeTool, setActiveTool] = useState<RightTool | null>(
    DEFAULT_PANEL_UI.rightTool,
  );
  const [bottomTool, setBottomTool] = useState<BottomTool | null>(
    DEFAULT_PANEL_UI.bottomTool,
  );

  const hydrate = useCallback(
    (state: {
      sidebarOpen?: boolean;
      rightTool?: string | null;
      bottomTool?: string | null;
    }) => {
      setSidebarOpen(state.sidebarOpen ?? DEFAULT_PANEL_UI.sidebarOpen);
      setActiveTool(parseRightTool(state.rightTool));
      setBottomTool(parseBottomTool(state.bottomTool));
    },
    [],
  );

  const reset = useCallback(() => {
    setSidebarOpen(DEFAULT_PANEL_UI.sidebarOpen);
    setActiveTool(DEFAULT_PANEL_UI.rightTool);
    setBottomTool(DEFAULT_PANEL_UI.bottomTool);
  }, []);

  const toggleSidebar = useCallback(() => {
    setSidebarOpen((open) => !open);
  }, []);

  const setSidebarOpenState = useCallback((open: boolean) => {
    setSidebarOpen(open);
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
    setSidebarOpen: setSidebarOpenState,
    activeTool,
    setRightTool,
    toggleRightTool,
    bottomTool,
    setBottomToolId,
    toggleBottomTool,
    hydrate,
    reset,
  };
}
