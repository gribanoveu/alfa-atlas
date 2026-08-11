import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { GeneralPrefs } from "../lib/prefs";
import { DEFAULT_GENERAL_PREFS } from "../lib/prefs";
import {
  readExternalTextFile,
  readProjectFile,
  writeExternalTextFile,
  writeProjectFile,
} from "../lib/project";
import { isImageAsset, isSupportedFile, monacoLanguageFor } from "../lib/supportedFiles";

export type EditorTabKind = "text" | "image";
export type EditorTabOrigin = "project" | "external";

export type EditorTab = {
  id: string;
  path: string;
  title: string;
  content: string;
  savedContent: string;
  language: string;
  dirty: boolean;
  /** Image tabs are preview-only — never read/write text content. */
  kind: EditorTabKind;
  /** Project tabs use docs-root-relative paths; external use absolute OS paths. */
  origin: EditorTabOrigin;
};

export type CursorPosition = {
  line: number;
  column: number;
};

export type EditorAutosavePrefs = Pick<
  GeneralPrefs,
  "autosaveEnabled" | "saveOnTabSwitch" | "autosaveDelayMs"
>;

function titleOf(relativePath: string): string {
  return relativePath.split(/[/\\]/).pop() ?? relativePath;
}

function externalTabId(absolutePath: string): string {
  return `external:${absolutePath}`;
}

function makeImageTab(relativePath: string): EditorTab {
  return {
    id: relativePath,
    path: relativePath,
    title: titleOf(relativePath),
    content: "",
    savedContent: "",
    language: "plaintext",
    dirty: false,
    kind: "image",
    origin: "project",
  };
}

function makeTextTab(relativePath: string, content: string): EditorTab {
  return {
    id: relativePath,
    path: relativePath,
    title: titleOf(relativePath),
    content,
    savedContent: content,
    language: monacoLanguageFor(relativePath),
    dirty: false,
    kind: "text",
    origin: "project",
  };
}

function makeExternalTextTab(absolutePath: string, content: string): EditorTab {
  return {
    id: externalTabId(absolutePath),
    path: absolutePath,
    title: titleOf(absolutePath),
    content,
    savedContent: content,
    language: monacoLanguageFor(absolutePath),
    dirty: false,
    kind: "text",
    origin: "external",
  };
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

type UseEditorTabsOptions = {
  onTabsChange?: (openTabs: string[], activeTab: string | null) => void;
  prefs?: EditorAutosavePrefs;
};

export function useEditorTabs(
  docsRoot: string | null,
  options: UseEditorTabsOptions = {},
) {
  const { onTabsChange } = options;
  const prefs = options.prefs ?? DEFAULT_GENERAL_PREFS;

  const [tabs, setTabs] = useState<EditorTab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const [cursor, setCursor] = useState<CursorPosition>({ line: 1, column: 1 });
  const [error, setError] = useState<string | null>(null);
  const [hydrated, setHydrated] = useState(false);
  const restoredForRoot = useRef<string | null>(null);

  // Navigation history (like IntelliJ IDEA back/forward)
  const MAX_HISTORY = 50;
  const [backStack, setBackStack] = useState<string[]>([]);
  const [forwardStack, setForwardStack] = useState<string[]>([]);

  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const activeTabIdRef = useRef(activeTabId);
  activeTabIdRef.current = activeTabId;
  const docsRootRef = useRef(docsRoot);
  docsRootRef.current = docsRoot;
  const prefsRef = useRef(prefs);
  prefsRef.current = prefs;
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const backStackRef = useRef(backStack);
  backStackRef.current = backStack;
  const forwardStackRef = useRef(forwardStack);
  forwardStackRef.current = forwardStack;

  const clearDebounce = useCallback(() => {
    if (debounceTimerRef.current !== null) {
      clearTimeout(debounceTimerRef.current);
      debounceTimerRef.current = null;
    }
  }, []);

  const saveTab = useCallback(async (id: string): Promise<boolean> => {
    const root = docsRootRef.current;
    const tab = tabsRef.current.find((t) => t.id === id);
    if (!tab) return false;
    if (tab.kind === "image") return true;
    if (!tab.dirty) return true;
    if (tab.origin === "project" && !root) return false;

    const contentToSave = tab.content;
    const path = tab.path;
    try {
      if (tab.origin === "external") {
        await writeExternalTextFile(path, contentToSave);
      } else {
        await writeProjectFile(root!, path, contentToSave);
      }
      setTabs((prev) => {
        const next = prev.map((t) => {
          if (t.id !== id) return t;
          return {
            ...t,
            savedContent: contentToSave,
            dirty: t.content !== contentToSave,
          };
        });
        tabsRef.current = next;
        return next;
      });
      setError(null);
      return true;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return false;
    }
  }, []);

  const flushDebounce = useCallback(async (): Promise<boolean> => {
    const hadPending = debounceTimerRef.current !== null;
    clearDebounce();
    if (!hadPending) return true;
    if (!prefsRef.current.autosaveEnabled) return true;
    const id = activeTabIdRef.current;
    if (!id) return true;
    const tab = tabsRef.current.find((t) => t.id === id);
    if (!tab?.dirty) return true;
    return saveTab(id);
  }, [clearDebounce, saveTab]);

  const switchToTab = useCallback(
    async (id: string) => {
      if (id === activeTabIdRef.current) return;
      await flushDebounce();
      const currentId = activeTabIdRef.current;
      if (currentId && prefsRef.current.saveOnTabSwitch) {
        const current = tabsRef.current.find((t) => t.id === currentId);
        if (current?.dirty) {
          const ok = await saveTab(currentId);
          if (!ok) return;
        }
      }
      setActiveTabId(id);
    },
    [flushDebounce, saveTab],
  );

  const prepareClose = useCallback(
    async (closing: EditorTab[]): Promise<boolean> => {
      if (closing.length === 0) return true;
      await flushDebounce();
      const { autosaveEnabled, saveOnTabSwitch } = prefsRef.current;
      if (autosaveEnabled || saveOnTabSwitch) {
        for (const tab of closing) {
          const latest = tabsRef.current.find((t) => t.id === tab.id);
          if (!latest?.dirty) continue;
          const ok = await saveTab(latest.id);
          if (!ok) return false;
        }
        return true;
      }
      const latestClosing = closing
        .map((tab) => tabsRef.current.find((t) => t.id === tab.id))
        .filter((t): t is EditorTab => Boolean(t));
      return confirmCloseDirty(latestClosing);
    },
    [flushDebounce, saveTab],
  );

  useEffect(() => {
    const rootForSession = docsRoot;
    setTabs([]);
    setActiveTabId(null);
    setCursor({ line: 1, column: 1 });
    setError(null);
    setHydrated(false);
    restoredForRoot.current = null;

    return () => {
      const hadPending = debounceTimerRef.current !== null;
      clearDebounce();
      if (
        !hadPending ||
        !prefsRef.current.autosaveEnabled
      ) {
        return;
      }
      const id = activeTabIdRef.current;
      if (!id) return;
      const tab = tabsRef.current.find((t) => t.id === id);
      if (!tab?.dirty || tab.kind === "image") return;
      if (tab.origin === "external") {
        void writeExternalTextFile(tab.path, tab.content);
        return;
      }
      if (!rootForSession) return;
      void writeProjectFile(rootForSession, tab.path, tab.content);
    };
  }, [docsRoot, clearDebounce]);

  useEffect(() => {
    if (!hydrated) return;
    const projectTabs = tabs.filter((t) => t.origin === "project");
    const activeProject =
      activeTabId &&
      projectTabs.some((t) => t.id === activeTabId)
        ? activeTabId
        : null;
    onTabsChange?.(
      projectTabs.map((t) => t.path),
      activeProject,
    );
  }, [tabs, activeTabId, onTabsChange, hydrated]);

  const activeTab = useMemo((): EditorTab | null => {
    if (!activeTabId) return null;
    return tabs.find((tab) => tab.id === activeTabId) ?? null;
  }, [tabs, activeTabId]);

  const pushToHistory = useCallback((path: string) => {
    const current = backStackRef.current;
    if (current.length > 0 && current[current.length - 1] === path) return;
    let next = [...current, path];
    if (next.length > MAX_HISTORY) next = next.slice(next.length - MAX_HISTORY);
    setBackStack(next);
  }, []);

  const openFile = useCallback(
    async (relativePath: string, opts?: { addToHistory?: boolean }) => {
      const addToHistory = opts?.addToHistory ?? true;
      if (!docsRoot) return;
      const existing = tabsRef.current.find(
        (tab) => tab.origin === "project" && tab.path === relativePath,
      );
      if (existing) {
        if (addToHistory && activeTabIdRef.current) {
          const current = tabsRef.current.find(
            (t) => t.id === activeTabIdRef.current,
          );
          if (current?.origin === "project") {
            pushToHistory(current.path);
          }
          setForwardStack([]);
        }
        await switchToTab(existing.id);
        // Сбрасываем ошибку от прошлой неудачной попытки открытия, иначе
        // тост «failed to resolve path» остаётся висеть после успешного
        // switchToTab.
        setError(null);
        return;
      }
      try {
        const tab = isImageAsset(relativePath)
          ? makeImageTab(relativePath)
          : makeTextTab(
              relativePath,
              await readProjectFile(docsRoot, relativePath),
            );
        await flushDebounce();
        const currentId = activeTabIdRef.current;
        if (currentId && prefsRef.current.saveOnTabSwitch) {
          const current = tabsRef.current.find((t) => t.id === currentId);
          if (current?.dirty) {
            const ok = await saveTab(currentId);
            if (!ok) return;
          }
        }
        if (addToHistory && currentId) {
          const current = tabsRef.current.find((t) => t.id === currentId);
          if (current?.origin === "project") {
            pushToHistory(current.path);
          }
          setForwardStack([]);
        }
        setTabs((prev) => {
          const next = [...prev, tab];
          tabsRef.current = next;
          return next;
        });
        setActiveTabId(tab.id);
        setError(null);
      } catch (e) {
        // Пробрасываем ошибку наружу, чтобы caller (например openDiagnostic)
        // мог её перехватить и не оставлять «failed to resolve path» в toast.
        setError(e instanceof Error ? e.message : String(e));
        throw e;
      }
    },
    [docsRoot, flushDebounce, saveTab, switchToTab, pushToHistory],
  );

  const openExternalFile = useCallback(
    async (absolutePath: string) => {
      if (!isSupportedFile(absolutePath)) {
        setError(
          `Формат не поддерживается для открытия: ${titleOf(absolutePath)}`,
        );
        return;
      }
      const id = externalTabId(absolutePath);
      const existing = tabsRef.current.find((tab) => tab.id === id);
      if (existing) {
        await switchToTab(existing.id);
        setError(null);
        return;
      }
      try {
        const content = await readExternalTextFile(absolutePath);
        const tab = makeExternalTextTab(absolutePath, content);
        await flushDebounce();
        const currentId = activeTabIdRef.current;
        if (currentId && prefsRef.current.saveOnTabSwitch) {
          const current = tabsRef.current.find((t) => t.id === currentId);
          if (current?.dirty) {
            const ok = await saveTab(currentId);
            if (!ok) return;
          }
        }
        setTabs((prev) => {
          const next = [...prev, tab];
          tabsRef.current = next;
          return next;
        });
        setActiveTabId(tab.id);
        setError(null);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [flushDebounce, saveTab, switchToTab],
  );

  const goBack = useCallback(async () => {
    const stack = backStackRef.current;
    if (stack.length === 0) return;
    const current = tabsRef.current.find(
      (t) => t.id === activeTabIdRef.current,
    );
    const currentPath =
      current?.origin === "project" ? current.path : null;
    const prevPath = stack[stack.length - 1];
    setBackStack((s) => s.slice(0, -1));
    if (currentPath) {
      setForwardStack((s) => {
        const next = [...s, currentPath];
        return next.length > MAX_HISTORY
          ? next.slice(next.length - MAX_HISTORY)
          : next;
      });
    }
    await openFile(prevPath, { addToHistory: false });
  }, [openFile]);

  const goForward = useCallback(async () => {
    const stack = forwardStackRef.current;
    if (stack.length === 0) return;
    const current = tabsRef.current.find(
      (t) => t.id === activeTabIdRef.current,
    );
    const currentPath =
      current?.origin === "project" ? current.path : null;
    const nextPath = stack[stack.length - 1];
    setForwardStack((s) => s.slice(0, -1));
    if (currentPath) {
      pushToHistory(currentPath);
    }
    await openFile(nextPath, { addToHistory: false });
  }, [openFile, pushToHistory]);

  const canGoBack = backStack.length > 0;
  const canGoForward = forwardStack.length > 0;

  const restoreTabs = useCallback(
    async (openTabs: string[], activeTab: string | null) => {
      if (!docsRoot) return;
      if (restoredForRoot.current === docsRoot) return;
      restoredForRoot.current = docsRoot;

      const restored: EditorTab[] = [];
      for (const path of openTabs) {
        try {
          if (isImageAsset(path)) {
            restored.push(makeImageTab(path));
            continue;
          }
          const content = await readProjectFile(docsRoot, path);
          restored.push(makeTextTab(path, content));
        } catch {
          // Skip missing files.
        }
      }

      const nextActive =
        (activeTab && restored.some((t) => t.id === activeTab)
          ? activeTab
          : null) ??
        restored[0]?.id ??
        null;

      setTabs(restored);
      tabsRef.current = restored;
      setActiveTabId(nextActive);
      setError(null);
      setHydrated(true);
    },
    [docsRoot],
  );

  const selectTab = useCallback(
    (id: string) => {
      if (id === activeTabIdRef.current) return;
      const current = tabsRef.current.find(
        (t) => t.id === activeTabIdRef.current,
      );
      if (current?.origin === "project") {
        pushToHistory(current.path);
        setForwardStack([]);
      }
      void switchToTab(id);
    },
    [pushToHistory, switchToTab],
  );

  const closeTab = useCallback(
    async (id: string) => {
      const tab = tabsRef.current.find((t) => t.id === id);
      if (!tab) return;
      const ok = await prepareClose([tab]);
      if (!ok) return;
      setTabs((prev) => {
        if (prev.length === 0) return prev;
        const index = prev.findIndex((t) => t.id === id);
        if (index < 0) return prev;
        const next = prev.filter((t) => t.id !== id);
        tabsRef.current = next;
        if (id === activeTabIdRef.current) {
          const fallback = next[Math.max(0, index - 1)] ?? next[0] ?? null;
          setActiveTabId(fallback?.id ?? null);
        }
        return next;
      });
    },
    [prepareClose],
  );

  const closeAllTabs = useCallback(async () => {
    const current = tabsRef.current;
    if (current.length === 0) return;
    const ok = await prepareClose(current);
    if (!ok) return;
    clearDebounce();
    setTabs([]);
    tabsRef.current = [];
    setActiveTabId(null);
  }, [clearDebounce, prepareClose]);

  const closeOtherTabs = useCallback(
    async (id: string) => {
      const keep = tabsRef.current.find((t) => t.id === id);
      if (!keep) return;
      const closing = tabsRef.current.filter((t) => t.id !== id);
      if (closing.length === 0) return;
      const ok = await prepareClose(closing);
      if (!ok) return;
      setTabs([keep]);
      tabsRef.current = [keep];
      setActiveTabId(keep.id);
    },
    [prepareClose],
  );

  const discardTabsUnder = useCallback((relativePath: string) => {
    const prefix = relativePath.replace(/[/\\]+$/, "") + "/";
    const isUnder = (tab: EditorTab) => {
      if (tab.origin !== "project") return false;
      const tabPath = tab.path;
      return (
        tabPath === relativePath ||
        tabPath.startsWith(prefix) ||
        tabPath.startsWith(relativePath + "\\")
      );
    };
    setTabs((prev) => {
      const next = prev.filter((t) => !isUnder(t));
      if (next.length === prev.length) return prev;
      tabsRef.current = next;
      const activeId = activeTabIdRef.current;
      if (activeId && !next.some((t) => t.id === activeId)) {
        const removedIndex = prev.findIndex((t) => t.id === activeId);
        const fallback =
          next[Math.max(0, removedIndex - 1)] ?? next[0] ?? null;
        setActiveTabId(fallback?.id ?? null);
      }
      return next;
    });
  }, []);

  const remapTabsUnder = useCallback(
    (oldPath: string, newPath: string) => {
      const prefix = oldPath.replace(/[/\\]+$/, "") + "/";
      const remap = (tabPath: string): string | null => {
        if (tabPath === oldPath) return newPath;
        if (tabPath.startsWith(prefix)) return newPath + tabPath.slice(prefix.length);
        if (tabPath.startsWith(oldPath + "\\")) {
          return newPath + tabPath.slice(oldPath.length);
        }
        return null;
      };
      setTabs((prev) => {
        let changed = false;
        const next = prev.map((tab) => {
          if (tab.origin !== "project") return tab;
          const remapped = remap(tab.path);
          if (!remapped) return tab;
          changed = true;
          const kind: EditorTabKind = isImageAsset(remapped) ? "image" : "text";
          return {
            ...tab,
            id: remapped,
            path: remapped,
            title: titleOf(remapped),
            language: monacoLanguageFor(remapped),
            kind,
            origin: "project" as const,
            ...(kind === "image"
              ? { content: "", savedContent: "", dirty: false as const }
              : {}),
          };
        });
        if (!changed) return prev;
        tabsRef.current = next;
        const activeId = activeTabIdRef.current;
        if (activeId) {
          const remappedActive = remap(activeId);
          if (remappedActive) setActiveTabId(remappedActive);
        }
        return next;
      });
    },
    [],
  );

  const updateActiveContent = useCallback(
    (content: string) => {
      if (!activeTabId) return;
      const active = tabsRef.current.find((t) => t.id === activeTabId);
      if (!active || active.kind === "image") return;
      setTabs((prev) => {
        const next = prev.map((tab) =>
          tab.id === activeTabId
            ? {
                ...tab,
                content,
                dirty: content !== tab.savedContent,
              }
            : tab,
        );
        tabsRef.current = next;
        return next;
      });

      clearDebounce();
      if (!prefsRef.current.autosaveEnabled) return;
      const delay = prefsRef.current.autosaveDelayMs;
      debounceTimerRef.current = setTimeout(() => {
        debounceTimerRef.current = null;
        const id = activeTabIdRef.current;
        if (!id) return;
        const tab = tabsRef.current.find((t) => t.id === id);
        if (!tab?.dirty || tab.kind === "image") return;
        void saveTab(id);
      }, delay);
    },
    [activeTabId, clearDebounce, saveTab],
  );

  const saveActive = useCallback(async () => {
    const id = activeTabIdRef.current;
    if (!id) return false;
    clearDebounce();
    return saveTab(id);
  }, [clearDebounce, saveTab]);

  const reset = useCallback(async () => {
    await flushDebounce();
    setTabs([]);
    tabsRef.current = [];
    setActiveTabId(null);
    setCursor({ line: 1, column: 1 });
    setError(null);
    setHydrated(false);
    restoredForRoot.current = null;
    setBackStack([]);
    setForwardStack([]);
  }, [flushDebounce]);

  const reloadTabFromDisk = useCallback(
    async (relativePath: string): Promise<boolean> => {
      const root = docsRootRef.current;
      const tab = tabsRef.current.find(
        (t) => t.origin === "project" && t.path === relativePath,
      );
      if (!root || !tab) return false;
      if (tab.kind === "image") return true;
      try {
        const content = await readProjectFile(root, relativePath);
        setTabs((prev) => {
          const next = prev.map((t) =>
            t.id === tab.id
              ? {
                  ...t,
                  content,
                  savedContent: content,
                  dirty: false,
                }
              : t,
          );
          tabsRef.current = next;
          return next;
        });
        return true;
      } catch {
        return false;
      }
    },
    [],
  );

  const saveAllDirtyTabs = useCallback(async (): Promise<boolean> => {
    await flushDebounce();
    for (const tab of tabsRef.current) {
      if (!tab.dirty) continue;
      const ok = await saveTab(tab.id);
      if (!ok) return false;
    }
    return true;
  }, [flushDebounce, saveTab]);

  const reloadAllOpenTabs = useCallback(async (): Promise<void> => {
    const root = docsRootRef.current;
    if (!root) return;
    const open = [...tabsRef.current];
    for (const tab of open) {
      if (tab.origin !== "project" || tab.kind === "image") continue;
      try {
        const content = await readProjectFile(root, tab.path);
        setTabs((prev) => {
          const next = prev.map((t) =>
            t.id === tab.id
              ? {
                  ...t,
                  content,
                  savedContent: content,
                  dirty: false,
                }
              : t,
          );
          tabsRef.current = next;
          return next;
        });
      } catch {
        // File may not exist on this branch; leave tab content as-is.
      }
    }
  }, []);

  return {
    tabs,
    activeTabId,
    activeTab,
    selectTab,
    closeTab,
    closeAllTabs,
    closeOtherTabs,
    discardTabsUnder,
    remapTabsUnder,
    openFile,
    openExternalFile,
    restoreTabs,
    updateActiveContent,
    saveActive,
    cursor,
    setCursor,
    error,
    reset,
    reloadTabFromDisk,
    saveAllDirtyTabs,
    reloadAllOpenTabs,
    goBack,
    goForward,
    canGoBack,
    canGoForward,
  };
}
