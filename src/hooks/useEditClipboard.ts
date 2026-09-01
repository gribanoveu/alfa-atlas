import { useCallback, useEffect, useMemo, useRef, type RefObject } from "react";
import type { editor as MonacoEditor } from "monaco-editor";
import { copyToClipboard, readClipboardText } from "../lib/clipboard";
import {
  applyFieldEdit,
  asTextField,
  availabilityFor,
  isFieldWritable,
  isInsideMonaco,
  isMonacoWritable,
  replaceMonacoSelections,
  selectionTextOf,
  type EditAvailability,
  type EditTarget,
  type TextField,
} from "../lib/editClipboard";

export type EditClipboard = {
  cut: () => void;
  copy: () => void;
  paste: () => void;
  /** Sampled by the menu when it opens — see `MenuBar`. */
  availability: () => EditAvailability;
};

/** Menu selector for the top menu bar, whose own clicks must not count as
 *  leaving the editor: opening «Правка» would otherwise clear the very target
 *  its items are about. */
const MENU_BAR = "nav.menu";

/** Cut/Copy/Paste for the «Правка» menu, acting on whatever was focused
 *  before the menu opened: the active Monaco editor, a plain text field, or —
 *  for Copy alone — a plain selection in a preview pane.
 *
 * The target is remembered on `focusin` rather than read from
 * `document.activeElement` at click time, because by then focus may already
 * have moved to the menu item. Selections survive blur in both Monaco and
 * text fields, so the remembered target still knows what is selected. */
export function useEditClipboard(
  activeEditorRef: RefObject<MonacoEditor.IStandaloneCodeEditor | null>,
): EditClipboard {
  const fieldRef = useRef<TextField | null>(null);
  const inMonacoRef = useRef(false);
  const selectionRef = useRef("");

  useEffect(() => {
    const onFocusIn = (event: FocusEvent) => {
      const el = event.target as Element | null;
      if (el?.closest?.(MENU_BAR)) {
        return;
      }
      if (isInsideMonaco(el)) {
        inMonacoRef.current = true;
        fieldRef.current = null;
        return;
      }
      inMonacoRef.current = false;
      fieldRef.current = asTextField(el);
    };

    // A selection outside a field (preview pane, chat) is collapsed by the
    // click that opens the menu, so remember it while it lasts and drop it
    // only once the user goes somewhere else.
    const onSelectionChange = () => {
      const text = window.getSelection()?.toString() ?? "";
      if (text) {
        selectionRef.current = text;
      }
    };

    const onPointerDown = (event: PointerEvent) => {
      if ((event.target as Element | null)?.closest?.(MENU_BAR)) {
        return;
      }
      selectionRef.current = "";
    };

    document.addEventListener("focusin", onFocusIn);
    document.addEventListener("selectionchange", onSelectionChange);
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("focusin", onFocusIn);
      document.removeEventListener("selectionchange", onSelectionChange);
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, []);

  const target = useCallback((): EditTarget => {
    if (inMonacoRef.current) {
      const editor = activeEditorRef.current;
      return editor ? { kind: "monaco", editor } : null;
    }
    const el = fieldRef.current;
    // The field may have been unmounted since it was focused (a closed
    // dialog, a switched tab) — writing into it would go nowhere.
    return el?.isConnected ? { kind: "field", el } : null;
  }, [activeEditorRef]);

  const write = useCallback((target: EditTarget, text: string) => {
    if (!target) {
      return;
    }
    if (target.kind === "monaco") {
      if (isMonacoWritable(target.editor)) {
        replaceMonacoSelections(target.editor, text);
      }
      return;
    }
    if (isFieldWritable(target.el)) {
      applyFieldEdit(target.el, text);
    }
  }, []);

  const availability = useCallback(
    () => availabilityFor(target(), selectionRef.current),
    [target],
  );

  const copy = useCallback(() => {
    const text = selectionTextOf(target(), selectionRef.current);
    if (!text) {
      return;
    }
    // Буфер недоступен — молча ничего не копируем, как и остальные
    // «скопировать» в приложении.
    void copyToClipboard(text).catch(() => {});
  }, [target]);

  const cut = useCallback(() => {
    const current = target();
    const text = selectionTextOf(current, selectionRef.current);
    if (!current || !text) {
      return;
    }

    void (async () => {
      try {
        await copyToClipboard(text);
      } catch {
        // Never delete text that failed to reach the clipboard.
        return;
      }
      write(current, "");
    })();
  }, [target, write]);

  const paste = useCallback(() => {
    const current = target();
    if (!current) {
      return;
    }

    void (async () => {
      let text = "";
      try {
        text = await readClipboardText();
      } catch {
        return;
      }
      if (text) {
        write(current, text);
      }
    })();
  }, [target, write]);

  return useMemo(
    () => ({ cut, copy, paste, availability }),
    [cut, copy, paste, availability],
  );
}
