import type { editor as MonacoEditor } from "monaco-editor";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DisplayTab } from "../components/Editor/EditorTabs";
import type { SpecsRepoInfo } from "../lib/openapi";
import type { useEditorTabs } from "./useEditorTabs";

type Deps = {
  editor: ReturnType<typeof useEditorTabs>;
  specsRepo: { info: SpecsRepoInfo | null };
};

/** The tab strip, which shows one thing `useEditorTabs` knows nothing about:
 * the API Explorer.
 *
 * That tab has no file behind it, so it cannot live in `useEditorTabs` — it
 * is a virtual entry appended to the strip, identified by the reserved id
 * `"openapi"`. Everything here exists to keep the two in one list: which
 * kind is active, and select/close routed to the right owner.
 *
 * Also carries the menu's Undo/Redo, which need the live Monaco instance.
 * That is a ref rather than state on purpose: swapping editors must not
 * re-render the app, only leave the right pointer behind for the next click. */
export function useEditorTabActions({ editor, specsRepo }: Deps) {
  const [openApiTabOpen, setOpenApiTabOpen] = useState(false);
  const [activeKind, setActiveKind] = useState<"file" | "openapi">("file");

  // Any real file tab becoming active (open/select/restore-on-load) hands
  // focus back to the file view — the API Explorer stays active only when
  // the user explicitly picked it, which doesn't touch `activeTabId`.
  useEffect(() => {
    setActiveKind("file");
  }, [editor.activeTabId]);

  const displayTabs: DisplayTab[] = useMemo(() => {
    const fileTabs: DisplayTab[] = editor.tabs.map((t) => ({
      id: t.id,
      title: t.title,
      dirty: t.dirty,
    }));
    if (!openApiTabOpen) return fileTabs;
    return [
      ...fileTabs,
      { id: "openapi", title: specsRepo.info?.title ?? "API Explorer", dirty: false },
    ];
  }, [editor.tabs, openApiTabOpen, specsRepo.info]);

  const selectTab = useCallback(
    (id: string) => {
      if (id === "openapi") {
        setActiveKind("openapi");
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
      void editor.closeTab(id);
    },
    [editor.closeTab],
  );

  const openApiExplorerTab = useCallback(() => {
    setOpenApiTabOpen(true);
    setActiveKind("openapi");
  }, []);

  const closeAllTabs = useCallback(() => {
    setOpenApiTabOpen(false);
    setActiveKind("file");
    void editor.closeAllTabs();
  }, [editor.closeAllTabs]);

  const closeOtherTabs = useCallback(
    (id: string) => {
      // "Close others" from the API Explorer keeps the Explorer, so it
      // closes every *file* tab rather than delegating to `closeOtherTabs`,
      // which would not know about this one.
      if (id === "openapi") {
        setActiveKind("openapi");
        void editor.closeAllTabs();
        return;
      }
      setOpenApiTabOpen(false);
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
    activeKind,
    setActiveKind,
    displayTabs,
    selectTab,
    closeTab,
    closeAllTabs,
    closeOtherTabs,
    openApiExplorerTab,
    onEditorInstanceChange,
    undo,
    redo,
  };
}
