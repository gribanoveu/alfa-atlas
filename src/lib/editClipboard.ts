import type { editor as MonacoEditor } from "monaco-editor";

/** Cut/Copy/Paste for the «Правка» menu.
 *
 * The menu is a React dropdown, not a native one, so the webview's own
 * clipboard commands never fire for it: `document.execCommand("cut"|"paste")`
 * is inert in a WKWebView, and Monaco's `clipboardPasteAction` leans on the
 * same browser plumbing. Every action here therefore goes through the Tauri
 * clipboard plugin and edits the target — the focused Monaco editor, or a
 * plain text field anywhere in the app — by hand.
 *
 * The DOM parts live here rather than in the hook so they can be tested
 * without React. */

export type TextField = HTMLInputElement | HTMLTextAreaElement;

export type EditTarget =
  | { kind: "monaco"; editor: MonacoEditor.IStandaloneCodeEditor }
  | { kind: "field"; el: TextField }
  | null;

export type EditAvailability = { cut: boolean; copy: boolean; paste: boolean };

export const NOTHING_AVAILABLE: EditAvailability = { cut: false, copy: false, paste: false };

/** Input types whose `selectionStart` is readable. `number`, `email` and
 *  friends throw on it, so they stay out of the menu's reach entirely. */
const SELECTABLE_INPUT_TYPES = new Set(["text", "search", "url", "tel", "password"]);

export function asTextField(el: Element | null | undefined): TextField | null {
  if (el instanceof HTMLTextAreaElement) {
    return el;
  }
  if (el instanceof HTMLInputElement && SELECTABLE_INPUT_TYPES.has(el.type)) {
    return el;
  }
  return null;
}

/** Monaco's focus lands on a hidden `textarea` inside `.monaco-editor`, which
 *  would otherwise pass for an ordinary field — check this first. */
export function isInsideMonaco(el: Element | null | undefined): boolean {
  return Boolean(el?.closest?.(".monaco-editor"));
}

export function isFieldWritable(el: TextField): boolean {
  return !el.disabled && !el.readOnly;
}

/** Browsers refuse to hand a password field's text to the clipboard; do the
 *  same, or the menu turns into a reveal button for stored tokens. */
export function isFieldReadable(el: TextField): boolean {
  return !(el instanceof HTMLInputElement && el.type === "password");
}

export function fieldSelectionText(el: TextField): string {
  const start = el.selectionStart ?? 0;
  const end = el.selectionEnd ?? 0;
  return start === end ? "" : el.value.slice(start, end);
}

export function spliceValue(
  value: string,
  start: number,
  end: number,
  insert: string,
): { value: string; caret: number } {
  return {
    value: value.slice(0, start) + insert + value.slice(end),
    caret: start + insert.length,
  };
}

/** React installs its own `value` setter on the element instance, so a plain
 *  `el.value = …` updates the DOM without React hearing about it and the next
 *  render puts the old text back. Going through the prototype setter is the
 *  standard way to make a synthetic edit look like a typed one. */
function setNativeValue(el: TextField, value: string): void {
  const proto =
    el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  if (setter) {
    setter.call(el, value);
  } else {
    el.value = value;
  }
}

/** Replaces the field's selection with `insert` (empty string for a cut) and
 *  tells React about it. */
export function applyFieldEdit(el: TextField, insert: string): void {
  const start = el.selectionStart ?? el.value.length;
  const end = el.selectionEnd ?? start;
  const next = spliceValue(el.value, start, end, insert);

  setNativeValue(el, next.value);
  el.setSelectionRange(next.caret, next.caret);
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.focus();
}

export function isMonacoWritable(editor: MonacoEditor.IStandaloneCodeEditor): boolean {
  return !editor.getRawOptions().readOnly;
}

export function monacoSelectionText(editor: MonacoEditor.IStandaloneCodeEditor): string {
  const model = editor.getModel();
  const selection = editor.getSelection();
  if (!model || !selection || selection.isEmpty()) {
    return "";
  }
  return model.getValueInRange(selection);
}

/** Writes `text` over every cursor's selection, as a single undo step. */
export function replaceMonacoSelections(
  editor: MonacoEditor.IStandaloneCodeEditor,
  text: string,
): void {
  const selections = editor.getSelections();
  if (!selections?.length) {
    return;
  }

  editor.pushUndoStop();
  editor.executeEdits(
    "menu",
    selections.map((range) => ({ range, text, forceMoveMarkers: true })),
  );
  editor.pushUndoStop();
  editor.focus();
}

/** What is copyable/pastable right now. `documentSelection` is the plain text
 *  selected outside any field — a preview pane, say — which only Copy uses. */
export function availabilityFor(
  target: EditTarget,
  documentSelection: string,
): EditAvailability {
  if (!target) {
    return { cut: false, copy: documentSelection.length > 0, paste: false };
  }

  if (target.kind === "monaco") {
    const writable = isMonacoWritable(target.editor);
    const selected = monacoSelectionText(target.editor).length > 0;
    return { cut: selected && writable, copy: selected, paste: writable };
  }

  const readable = isFieldReadable(target.el);
  const writable = isFieldWritable(target.el);
  const selected = fieldSelectionText(target.el).length > 0;
  return { cut: selected && readable && writable, copy: selected && readable, paste: writable };
}

/** The text Copy/Cut would put on the clipboard for `target`. */
export function selectionTextOf(target: EditTarget, documentSelection: string): string {
  if (!target) {
    return documentSelection;
  }
  if (target.kind === "monaco") {
    return monacoSelectionText(target.editor);
  }
  return isFieldReadable(target.el) ? fieldSelectionText(target.el) : "";
}
