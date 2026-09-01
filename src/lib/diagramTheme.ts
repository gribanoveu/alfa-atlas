import { useSyncExternalStore } from "react";
import {
  DEFAULT_DIAGRAM_THEME,
  getGeneralPrefs,
  setGeneralPrefs,
  type DiagramTheme,
} from "./prefs";

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

/** The same switch as Настройки → Оформление → «Тема диаграмм», reachable
 *  from a diagram's own toolbar.
 *
 *  Applies the palette at once — every open diagram redraws through the
 *  store — and saves it, so the button and the settings dialog cannot drift
 *  apart. The write re-reads prefs first instead of patching a copy held in
 *  React state: this is called from viewers three subtrees deep, which have
 *  no business carrying the other twenty preferences around just to save one
 *  of them (`useNotificationsLayout` persists its two flags the same way). */
export function chooseDiagramTheme(theme: DiagramTheme): void {
  setDiagramTheme(theme);
  void getGeneralPrefs()
    .then((prefs) => setGeneralPrefs({ ...prefs, diagramTheme: theme }))
    .catch(() => {
      // Не сохранилось — переключение всё равно применено к текущей сессии.
    });
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
