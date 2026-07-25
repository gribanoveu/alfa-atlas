import { useCallback, useEffect, useMemo, useState } from "react";
import {
  readProjectFile,
  writeProjectFile,
} from "../lib/project";
import { monacoLanguageFor } from "../lib/supportedFiles";

export type EditorTab = {
  id: string;
  path: string;
  title: string;
  content: string;
  savedContent: string;
  language: string;
  dirty: boolean;
};

export type CursorPosition = {
  line: number;
  column: number;
};

function titleOf(relativePath: string): string {
  return relativePath.split(/[/\\]/).pop() ?? relativePath;
}

function confirmCloseDirty(closing: EditorTab[]): boolean {
  if (!closing.some((tab) => tab.dirty)) return true;
  if (closing.length === 1) {
    return window.confirm(
      `Файл «${closing[0].title}» изменён. Закрыть без сохранения?`,
    );
  }
  return window.confirm(
    "Есть несохранённые изменения. Закрыть без сохранения?",
  );
}

export function useEditorTabs(docsRoot: string | null) {
  const [tabs, setTabs] = useState<EditorTab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const [cursor, setCursor] = useState<CursorPosition>({ line: 1, column: 1 });
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setTabs([]);
    setActiveTabId(null);
    setCursor({ line: 1, column: 1 });
    setError(null);
  }, [docsRoot]);

  const activeTab = useMemo((): EditorTab | null => {
    if (!activeTabId) return null;
    return tabs.find((tab) => tab.id === activeTabId) ?? null;
  }, [tabs, activeTabId]);

  const openFile = useCallback(
    async (relativePath: string) => {
      if (!docsRoot) return;
      const existing = tabs.find((tab) => tab.path === relativePath);
      if (existing) {
        setActiveTabId(existing.id);
        return;
      }
      try {
        const content = await readProjectFile(docsRoot, relativePath);
        const tab: EditorTab = {
          id: relativePath,
          path: relativePath,
          title: titleOf(relativePath),
          content,
          savedContent: content,
          language: monacoLanguageFor(relativePath),
          dirty: false,
        };
        setTabs((prev) => [...prev, tab]);
        setActiveTabId(tab.id);
        setError(null);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [docsRoot, tabs],
  );

  const selectTab = useCallback((id: string) => {
    setActiveTabId(id);
  }, []);

  const closeTab = useCallback(
    (id: string) => {
      const tab = tabs.find((t) => t.id === id);
      if (!tab) return;
      if (!confirmCloseDirty([tab])) return;
      setTabs((prev) => {
        if (prev.length === 0) return prev;
        const index = prev.findIndex((t) => t.id === id);
        if (index < 0) return prev;
        const next = prev.filter((t) => t.id !== id);
        if (id === activeTabId) {
          const fallback = next[Math.max(0, index - 1)] ?? next[0] ?? null;
          setActiveTabId(fallback?.id ?? null);
        }
        return next;
      });
    },
    [activeTabId, tabs],
  );

  const closeAllTabs = useCallback(() => {
    if (tabs.length === 0) return;
    if (!confirmCloseDirty(tabs)) return;
    setTabs([]);
    setActiveTabId(null);
  }, [tabs]);

  const closeOtherTabs = useCallback(
    (id: string) => {
      const keep = tabs.find((t) => t.id === id);
      if (!keep) return;
      const closing = tabs.filter((t) => t.id !== id);
      if (closing.length === 0) return;
      if (!confirmCloseDirty(closing)) return;
      setTabs([keep]);
      setActiveTabId(keep.id);
    },
    [tabs],
  );

  const updateActiveContent = useCallback(
    (content: string) => {
      if (!activeTabId) return;
      setTabs((prev) =>
        prev.map((tab) =>
          tab.id === activeTabId
            ? {
                ...tab,
                content,
                dirty: content !== tab.savedContent,
              }
            : tab,
        ),
      );
    },
    [activeTabId],
  );

  const saveActive = useCallback(async () => {
    if (!docsRoot || !activeTab) return false;
    if (!activeTab.dirty) return true;
    try {
      await writeProjectFile(docsRoot, activeTab.path, activeTab.content);
      setTabs((prev) =>
        prev.map((tab) =>
          tab.id === activeTab.id
            ? {
                ...tab,
                savedContent: tab.content,
                dirty: false,
              }
            : tab,
        ),
      );
      setError(null);
      return true;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return false;
    }
  }, [activeTab, docsRoot]);

  const reset = useCallback(() => {
    setTabs([]);
    setActiveTabId(null);
    setCursor({ line: 1, column: 1 });
    setError(null);
  }, []);

  return {
    tabs,
    activeTabId,
    activeTab,
    selectTab,
    closeTab,
    closeAllTabs,
    closeOtherTabs,
    openFile,
    updateActiveContent,
    saveActive,
    cursor,
    setCursor,
    error,
    reset,
  };
}
