import { useSyncExternalStore } from "react";
import { DEFAULT_DIAGRAM_THEME, type DiagramTheme } from "./prefs";

/** The current diagram palette, as a tiny subscribable store.
 *
 * A store rather than a prop because of who needs it: `AscMermaid` renders
 * from three different places (a `[mermaid]` block inside an AsciiDoc
 * preview, a standalone `.mmd` file, and the assistant's visualization
 * tab), and threading a theme prop down all three chains — through
 * `AsciiDocPreview` and `AscBlockList`, which otherwise have no interest in
 * it — would be a lot of plumbing for one value that is genuinely global.
 *
 * It also has to *re-render open diagrams* when it changes, which a plain
 * module-level variable would not: `useDiagramTheme` puts the value in
 * React's dependency graph, so flipping the setting redraws whatever is on
 * screen instead of waiting for the next navigation.
 *
 * `App` owns the write, from the persisted `GeneralPrefs`. */

let currentTheme: DiagramTheme = DEFAULT_DIAGRAM_THEME;
const listeners = new Set<() => void>();

export function getDiagramTheme(): DiagramTheme {
  return currentTheme;
}

export function setDiagramTheme(theme: DiagramTheme): void {
  if (theme === currentTheme) return;
  currentTheme = theme;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Subscribes a component to the diagram palette. */
export function useDiagramTheme(): DiagramTheme {
  // Same value server- and client-side: there is no SSR here, and the
  // getter is already a plain module read.
  return useSyncExternalStore(subscribe, getDiagramTheme, getDiagramTheme);
}
