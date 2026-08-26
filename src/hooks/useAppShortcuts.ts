import { useEffect, useRef } from "react";
import type { useEditorTabs } from "./useEditorTabs";
import type { useGitPanel } from "./useGitPanel";

type Deps = {
  hasProject: boolean;
  editor: ReturnType<typeof useEditorTabs>;
  git: ReturnType<typeof useGitPanel>;
  openDocsSearch: () => void;
};

/** Window-level shortcuts, and the git refresh that follows a save.
 *
 * These live on `window` rather than in Monaco because they must work
 * wherever focus happens to be — the file tree, the assistant panel, a
 * settings field. The editor's own keybindings only fire while it has focus.
 *
 * The save shortcut and autosave both end with dirty flags clearing, and git
 * has to be told either way; watching the dirty count catches both without
 * the autosave path having to know about git at all. */
export function useAppShortcuts({ hasProject, editor, git, openDocsSearch }: Deps) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;

      if (mod && event.key.toLowerCase() === "s") {
        // Claimed even without a project, so the browser's own "save page"
        // never appears over the app.
        event.preventDefault();
        if (hasProject) {
          void editor.saveActive().then((ok) => {
            if (ok) git.scheduleRefresh();
          });
        }
        return;
      }

      if (mod && event.shiftKey && event.key.toLowerCase() === "f") {
        event.preventDefault();
        if (hasProject) openDocsSearch();
        return;
      }

      if (mod && event.altKey) {
        if (event.key === "ArrowLeft") {
          event.preventDefault();
          void editor.goBack();
          return;
        }
        if (event.key === "ArrowRight") {
          event.preventDefault();
          void editor.goForward();
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    editor.saveActive,
    editor.goBack,
    editor.goForward,
    git.scheduleRefresh,
    hasProject,
    openDocsSearch,
  ]);

  /** Refreshes git once the dirty count *drops* — a save landed, whether the
   * user pressed the shortcut or autosave got there first. Rising counts are
   * ignored: typing has not changed anything on disk yet. */
  const prevDirtyCount = useRef(0);
  useEffect(() => {
    const dirtyCount = editor.tabs.filter((t) => t.dirty).length;
    if (prevDirtyCount.current > 0 && dirtyCount < prevDirtyCount.current) {
      git.scheduleRefresh();
    }
    prevDirtyCount.current = dirtyCount;
  }, [editor.tabs, git.scheduleRefresh]);
}
