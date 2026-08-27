import type { editor as MonacoEditor } from "monaco-editor";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DisplayTab } from "../components/Editor/EditorTabs";
import {
  findUtility,
  utilityIdFromTabId,
  utilityTabId,
  type UtilityId,
} from "../data/utilities";
import type { SpecsRepoInfo } from "../lib/openapi";
import type { useEditorTabs } from "./useEditorTabs";

type Deps = {
  editor: ReturnType<typeof useEditorTabs>;
  specsRepo: { info: SpecsRepoInfo | null };
};

/** Which pane the editor column shows: a real file tab, or one of the
 * pseudo-tabs that have no file behind them. */
export type EditorPaneKind = "file" | "openapi" | "utility";

/** The tab strip, which shows two things `useEditorTabs` knows nothing about:
 * the API Explorer and the utilities (Unixtime converter and friends).
 *
 * Those tabs have no file behind them, so they cannot live in
 * `useEditorTabs` — they are virtual entries appended to the strip, under the
 * reserved id `"openapi"` and the `utility:` id prefix. Everything here exists
 * to keep them in one list: which kind is active, and select/close routed to
 * the right owner.
 *
 * Also carries the menu's Undo/Redo, which need the live Monaco instance.
 * That is a ref rather than state on purpose: swapping editors must not
 * re-render the app, only leave the right pointer behind for the next click. */
export function useEditorTabActions({ editor, specsRepo }: Deps) {
  const [openApiTabOpen, setOpenApiTabOpen] = useState(false);
  const [openUtilities, setOpenUtilities] = useState<UtilityId[]>([]);
  const [activeUtility, setActiveUtility] = useState<UtilityId | null>(null);
  const [activeKind, setActiveKind] = useState<EditorPaneKind>("file");

  // Any real file tab becoming active (open/select/restore-on-load) hands
  // focus back to the file view — the pseudo-tabs stay active only when the
  // user explicitly picked one, which doesn't touch `activeTabId`.
  useEffect(() => {
    setActiveKind("file");
  }, [editor.activeTabId]);

  const displayTabs: DisplayTab[] = useMemo(() => {
    const fileTabs: DisplayTab[] = editor.tabs.map((t) => ({
      id: t.id,
      title: t.title,
      dirty: t.dirty,
    }));
    const utilityTabs: DisplayTab[] = openUtilities.map((id) => ({
      id: utilityTabId(id),
      title: findUtility(id)?.title ?? id,
      dirty: false,
    }));
    if (!openApiTabOpen) return [...fileTabs, ...utilityTabs];
    return [
      ...fileTabs,
      { id: "openapi", title: specsRepo.info?.title ?? "API Explorer", dirty: false },
      ...utilityTabs,
    ];
  }, [editor.tabs, openApiTabOpen, openUtilities, specsRepo.info]);

  const selectTab = useCallback(
    (id: string) => {
      if (id === "openapi") {
        setActiveKind("openapi");
        return;
      }
      const utilityId = utilityIdFromTabId(id);
      if (utilityId) {
        setActiveUtility(utilityId);
        setActiveKind("utility");
        return;
      }
      editor.selectTab(id);
    },
    [editor.selectTab],
  );

  const closeTab = useCallback(
    (id: string) => {
      if (id === "openapi") {
        setOpenApiTabOpen(false);
        setActiveKind("file");
        return;
      }
      const utilityId = utilityIdFromTabId(id);
      if (utilityId) {
        setOpenUtilities((prev) => prev.filter((open) => open !== utilityId));
        // Закрытие неактивной вкладки утилиты не уводит фокус с текущей.
        if (activeUtility === utilityId) {
          setActiveUtility(null);
          setActiveKind("file");
        }
        return;
      }
      void editor.closeTab(id);
    },
    [editor.closeTab, activeUtility],
  );

  /** Открывает утилиту вкладкой (или переключается на уже открытую). */
  const openUtilityTab = useCallback((id: UtilityId) => {
    setOpenUtilities((prev) => (prev.includes(id) ? prev : [...prev, id]));
    setActiveUtility(id);
    setActiveKind("utility");
  }, []);

  const openApiExplorerTab = useCallback(() => {
    setOpenApiTabOpen(true);
    setActiveKind("openapi");
  }, []);

  const closeAllTabs = useCallback(() => {
    setOpenApiTabOpen(false);
    setOpenUtilities([]);
    setActiveUtility(null);
    setActiveKind("file");
    void editor.closeAllTabs();
  }, [editor.closeAllTabs]);

  const closeOtherTabs = useCallback(
    (id: string) => {
      // "Close others" from a pseudo-tab keeps that tab, so it closes every
      // *file* tab rather than delegating to `closeOtherTabs`, which would
      // not know about it.
      if (id === "openapi") {
        setOpenUtilities([]);
        setActiveUtility(null);
        setActiveKind("openapi");
        void editor.closeAllTabs();
        return;
      }
      const utilityId = utilityIdFromTabId(id);
      if (utilityId) {
        setOpenApiTabOpen(false);
        setOpenUtilities([utilityId]);
        setActiveUtility(utilityId);
        setActiveKind("utility");
        void editor.closeAllTabs();
        return;
      }
      setOpenApiTabOpen(false);
      setOpenUtilities([]);
      setActiveUtility(null);
      void editor.closeOtherTabs(id);
    },
    [editor.closeAllTabs, editor.closeOtherTabs],
  );

  const activeEditorRef = useRef<MonacoEditor.IStandaloneCodeEditor | null>(null);
  const onEditorInstanceChange = useCallback(
    (instance: MonacoEditor.IStandaloneCodeEditor | null) => {
      activeEditorRef.current = instance;
    },
    [],
  );

  const runEditorCommand = useCallback((command: "undo" | "redo") => {
    const instance = activeEditorRef.current;
    if (!instance) return;
    instance.trigger("menu", command, null);
    instance.focus();
  }, []);

  const undo = useCallback(() => runEditorCommand("undo"), [runEditorCommand]);
  const redo = useCallback(() => runEditorCommand("redo"), [runEditorCommand]);

  return {
    openApiTabOpen,
    setOpenApiTabOpen,
    activeUtility,
    activeKind,
    setActiveKind,
    displayTabs,
    selectTab,
    closeTab,
    closeAllTabs,
    closeOtherTabs,
    openApiExplorerTab,
    openUtilityTab,
    onEditorInstanceChange,
    undo,
    redo,
  };
}
