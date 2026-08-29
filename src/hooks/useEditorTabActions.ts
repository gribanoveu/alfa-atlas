import type { editor as MonacoEditor } from "monaco-editor";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DisplayTab } from "../components/Editor/EditorTabs";
import {
  findUtility,
  utilityIdFromTabId,
  utilityTabId,
  type UtilityId,
} from "../data/utilities";
import { artifactIdFromTabId, artifactTabId } from "../lib/artifactTabs";
import { visualIdFromTabId, visualTabId, type Visual } from "../lib/visuals";
import type { SpecsRepoInfo } from "../lib/openapi";
import type { useEditorTabs } from "./useEditorTabs";

type Deps = {
  editor: ReturnType<typeof useEditorTabs>;
  specsRepo: { info: SpecsRepoInfo | null };
};

/** Which pane the editor column shows: a real file tab, or one of the
 * pseudo-tabs that have no file behind them. */
export type EditorPaneKind = "file" | "openapi" | "utility" | "artifact" | "visual";

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
  // Artifacts are pseudo-tabs like the utilities rather than virtual file
  // tabs like plans: a plan is markdown and belongs among the Monaco tabs,
  // but an artifact is a form, and none of `useEditorTabs`' machinery
  // (autosave to disk, language detection, git gutter, session restore)
  // applies to it. Unlike a utility, though, each one has an identity and
  // unsaved state, so the strip needs a title and a dirty flag per open id.
  const [openArtifacts, setOpenArtifacts] = useState<string[]>([]);
  const [activeArtifact, setActiveArtifact] = useState<string | null>(null);
  const [artifactTitles, setArtifactTitles] = useState<Record<string, string>>({});
  const [artifactDirty, setArtifactDirty] = useState<Record<string, boolean>>({});
  // Visualizations are pseudo-tabs too, but the simplest kind: read-only,
  // so there is no dirty flag, and *not backed by any store*, so the tab
  // has to hold the payload itself rather than an id it could reload from
  // (see `src/lib/visuals.ts`). That is why this is `Visual[]` where the
  // artifacts above are `string[]`.
  const [openVisuals, setOpenVisuals] = useState<Visual[]>([]);
  const [activeVisualId, setActiveVisualId] = useState<string | null>(null);
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
    const artifactTabs: DisplayTab[] = openArtifacts.map((id) => ({
      id: artifactTabId(id),
      title: artifactTitles[id] ?? "Артефакт",
      dirty: artifactDirty[id] ?? false,
    }));
    const visualTabs: DisplayTab[] = openVisuals.map((visual) => ({
      id: visualTabId(visual.id),
      title: visual.title,
      dirty: false,
    }));
    if (!openApiTabOpen) return [...fileTabs, ...utilityTabs, ...artifactTabs, ...visualTabs];
    return [
      ...fileTabs,
      { id: "openapi", title: specsRepo.info?.title ?? "API Explorer", dirty: false },
      ...utilityTabs,
      ...artifactTabs,
      ...visualTabs,
    ];
  }, [
    editor.tabs,
    openApiTabOpen,
    openUtilities,
    openArtifacts,
    artifactTitles,
    artifactDirty,
    openVisuals,
    specsRepo.info,
  ]);

  /** Drops a closed artifact's strip metadata so a reopened one starts from
   * the record on disk rather than a stale title/dirty flag. */
  const forgetArtifact = useCallback((artifactId: string) => {
    setArtifactTitles((prev) => {
      if (!(artifactId in prev)) return prev;
      const { [artifactId]: _dropped, ...rest } = prev;
      return rest;
    });
    setArtifactDirty((prev) => {
      if (!(artifactId in prev)) return prev;
      const { [artifactId]: _dropped, ...rest } = prev;
      return rest;
    });
  }, []);

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
      const artifactId = artifactIdFromTabId(id);
      if (artifactId) {
        setActiveArtifact(artifactId);
        setActiveKind("artifact");
        return;
      }
      const visualId = visualIdFromTabId(id);
      if (visualId) {
        setActiveVisualId(visualId);
        setActiveKind("visual");
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
      const artifactId = artifactIdFromTabId(id);
      if (artifactId) {
        setOpenArtifacts((prev) => prev.filter((open) => open !== artifactId));
        forgetArtifact(artifactId);
        if (activeArtifact === artifactId) {
          setActiveArtifact(null);
          setActiveKind("file");
        }
        return;
      }
      const visualId = visualIdFromTabId(id);
      if (visualId) {
        setOpenVisuals((prev) => prev.filter((open) => open.id !== visualId));
        if (activeVisualId === visualId) {
          setActiveVisualId(null);
          setActiveKind("file");
        }
        return;
      }
      void editor.closeTab(id);
    },
    [editor.closeTab, activeUtility, activeArtifact, activeVisualId, forgetArtifact],
  );

  /** Открывает утилиту вкладкой (или переключается на уже открытую). */
  const openUtilityTab = useCallback((id: UtilityId) => {
    setOpenUtilities((prev) => (prev.includes(id) ? prev : [...prev, id]));
    setActiveUtility(id);
    setActiveKind("utility");
  }, []);

  /** Opens an artifact's builder tab (or switches to the already-open one). */
  const openArtifactTab = useCallback((artifactId: string) => {
    setOpenArtifacts((prev) => (prev.includes(artifactId) ? prev : [...prev, artifactId]));
    setActiveArtifact(artifactId);
    setActiveKind("artifact");
  }, []);

  /** Opens a visualization's tab (or switches to the already-open one).
   *  Re-opening an id that is already open replaces its payload rather than
   *  keeping the old one — the assistant redrawing a diagram under the same
   *  id should update the tab, not be silently ignored. */
  const openVisualTab = useCallback((visual: Visual) => {
    setOpenVisuals((prev) => {
      const index = prev.findIndex((open) => open.id === visual.id);
      if (index === -1) return [...prev, visual];
      const next = [...prev];
      next[index] = visual;
      return next;
    });
    setActiveVisualId(visual.id);
    setActiveKind("visual");
  }, []);

  const setArtifactTitle = useCallback((artifactId: string, title: string) => {
    setArtifactTitles((prev) =>
      prev[artifactId] === title ? prev : { ...prev, [artifactId]: title },
    );
  }, []);

  const setArtifactDirtyFlag = useCallback((artifactId: string, dirty: boolean) => {
    setArtifactDirty((prev) => (prev[artifactId] === dirty ? prev : { ...prev, [artifactId]: dirty }));
  }, []);

  const openApiExplorerTab = useCallback(() => {
    setOpenApiTabOpen(true);
    setActiveKind("openapi");
  }, []);

  const closeAllTabs = useCallback(() => {
    setOpenApiTabOpen(false);
    setOpenUtilities([]);
    setActiveUtility(null);
    setOpenArtifacts([]);
    setActiveArtifact(null);
    setArtifactTitles({});
    setArtifactDirty({});
    setOpenVisuals([]);
    setActiveVisualId(null);
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
        setOpenArtifacts([]);
        setActiveArtifact(null);
        setOpenVisuals([]);
        setActiveVisualId(null);
        setActiveKind("openapi");
        void editor.closeAllTabs();
        return;
      }
      const utilityId = utilityIdFromTabId(id);
      if (utilityId) {
        setOpenApiTabOpen(false);
        setOpenUtilities([utilityId]);
        setActiveUtility(utilityId);
        setOpenArtifacts([]);
        setActiveArtifact(null);
        setOpenVisuals([]);
        setActiveVisualId(null);
        setActiveKind("utility");
        void editor.closeAllTabs();
        return;
      }
      const artifactId = artifactIdFromTabId(id);
      if (artifactId) {
        setOpenApiTabOpen(false);
        setOpenUtilities([]);
        setActiveUtility(null);
        setOpenArtifacts([artifactId]);
        setActiveArtifact(artifactId);
        setOpenVisuals([]);
        setActiveVisualId(null);
        setActiveKind("artifact");
        void editor.closeAllTabs();
        return;
      }
      const visualId = visualIdFromTabId(id);
      if (visualId) {
        setOpenApiTabOpen(false);
        setOpenUtilities([]);
        setActiveUtility(null);
        setOpenArtifacts([]);
        setActiveArtifact(null);
        setOpenVisuals((prev) => prev.filter((open) => open.id === visualId));
        setActiveVisualId(visualId);
        setActiveKind("visual");
        void editor.closeAllTabs();
        return;
      }
      setOpenApiTabOpen(false);
      setOpenUtilities([]);
      setActiveUtility(null);
      setOpenArtifacts([]);
      setActiveArtifact(null);
      setOpenVisuals([]);
      setActiveVisualId(null);
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
    activeArtifact,
    openArtifactTab,
    setArtifactTitle,
    setArtifactDirtyFlag,
    activeVisual: openVisuals.find((visual) => visual.id === activeVisualId) ?? null,
    openVisualTab,
    onEditorInstanceChange,
    undo,
    redo,
  };
}
