import { useCallback, useMemo, useState } from "react";

export type RightTool = "assistant" | "asciidoc" | "git";
export type BottomTool = "suggestions" | "formatting";

export type EditorTab = {
  id: string;
  title: string;
  content: string;
  language: string;
  dirty: boolean;
};

export type CursorPosition = {
  line: number;
  column: number;
};

const INITIAL_TAB: EditorTab = {
  id: "untitled-1",
  title: "Untitled-1",
  content: "",
  language: "plaintext",
  dirty: false,
};

export function useWorkspaceLayout() {
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [activeTool, setActiveTool] = useState<RightTool | null>("assistant");
  const [bottomTool, setBottomTool] = useState<BottomTool | null>(null);
  const [tabs, setTabs] = useState<EditorTab[]>([INITIAL_TAB]);
  const [activeTabId, setActiveTabId] = useState(INITIAL_TAB.id);
  const [cursor, setCursor] = useState<CursorPosition>({ line: 1, column: 1 });

  const activeTab = useMemo((): EditorTab => {
    return tabs.find((tab) => tab.id === activeTabId) ?? tabs[0] ?? INITIAL_TAB;
  }, [tabs, activeTabId]);

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

  const selectTab = useCallback((id: string) => {
    setActiveTabId(id);
  }, []);

  const closeTab = useCallback(
    (id: string) => {
      setTabs((prev) => {
        if (prev.length <= 1) return prev;
        const index = prev.findIndex((tab) => tab.id === id);
        if (index < 0) return prev;
        const next = prev.filter((tab) => tab.id !== id);
        if (id === activeTabId) {
          const fallback = next[Math.max(0, index - 1)] ?? next[0];
          setActiveTabId(fallback.id);
        }
        return next;
      });
    },
    [activeTabId],
  );

  const updateActiveContent = useCallback(
    (content: string) => {
      setTabs((prev) =>
        prev.map((tab) =>
          tab.id === activeTabId
            ? { ...tab, content, dirty: content.length > 0 }
            : tab,
        ),
      );
    },
    [activeTabId],
  );

  return {
    sidebarOpen,
    toggleSidebar,
    activeTool,
    setRightTool,
    toggleRightTool,
    bottomTool,
    setBottomToolId,
    toggleBottomTool,
    tabs,
    activeTabId,
    activeTab,
    selectTab,
    closeTab,
    updateActiveContent,
    cursor,
    setCursor,
  };
}
