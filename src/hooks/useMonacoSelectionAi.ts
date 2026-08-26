import type * as Monaco from "monaco-editor";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import { toMessage } from "../lib/errors";
import { llmChatOnce } from "../lib/llm";
import {
  buildSelectionAiMessages,
  SELECTION_AI_MAX_CHARS,
  stripCodeFence,
  type SelectionAiAction,
} from "../lib/selectionAiPrompts";
import type { EditorTab } from "./useEditorTabs";
import type { EditorViewMode } from "../types/viewMode";

export type SelectionAiPhase =
  | "hidden"
  | "toolbar"
  | "loading"
  | "preview"
  | "error";

export type SelectionAiPosition = {
  /** Anchor for the toolbar (clamped so translate(-50%) stays in-bounds). */
  top: number;
  left: number;
  /** Horizontal anchor for the preview card (may differ when widths differ). */
  previewLeft: number;
  /** Vertical anchor for the preview card. */
  previewTop: number;
  /** Whether the toolbar should sit above (`translateY(-100%)`) or below. */
  toolbarPlacement: "above" | "below";
  /** Whether the preview card opens above or below its anchor. */
  previewPlacement: "above" | "below";
};

export type SelectionAiUiState = {
  phase: SelectionAiPhase;
  position: SelectionAiPosition | null;
  selectedText: string;
  suggestedText: string | null;
  error: string | null;
  activeAction: SelectionAiAction | null;
  customPromptOpen: boolean;
  /** Whether the collapsed toolbar has been expanded to show all actions
   * («Больше» clicked) — collapsed shows only «Добавить в чат» + «Больше». */
  moreExpanded: boolean;
  tooLong: boolean;
  llmReady: boolean;
};

type UseMonacoSelectionAiOptions = {
  monaco: typeof Monaco | null;
  editor: Monaco.editor.IStandaloneCodeEditor | null;
  activeTab: EditorTab | null;
  viewMode: EditorViewMode;
  providerId: string | null;
  llmReady: boolean;
  onContentChange: (content: string) => void;
  /** Called by the toolbar's «Добавить в чат» with the stored selection and
   * its file's docs-root-relative path (`null` for virtual tabs). */
  onAddToChat?: (text: string, filePath: string | null) => void;
};

/** Delay after pointer-up / keyboard selection settle before showing the bar. */
const SHOW_DEBOUNCE_MS = 450;
const TOOLBAR_OFFSET_Y = 8;
const EDGE_PAD = 10;
/** Fallback half-width until the toolbar is measured (~320px toolbar). */
const TOOLBAR_HALF_FALLBACK = 160;
const PREVIEW_HALF_FALLBACK = 180;
/** Estimated preview height used before the card is measured. */
const PREVIEW_HEIGHT_FALLBACK = 200;

type StoredSelection = {
  range: Monaco.IRange;
  text: string;
};

type HalfWidths = { toolbar: number; preview: number };

function clampCenter(
  center: number,
  containerWidth: number,
  halfWidth: number,
): number {
  const min = halfWidth + EDGE_PAD;
  const max = containerWidth - halfWidth - EDGE_PAD;
  if (max <= min) return Math.max(EDGE_PAD, containerWidth / 2);
  return Math.min(max, Math.max(min, center));
}

function computePosition(
  editor: Monaco.editor.IStandaloneCodeEditor,
  selection: Monaco.Selection,
  halfWidths: HalfWidths,
  previewHeight = PREVIEW_HEIGHT_FALLBACK,
): SelectionAiPosition | null {
  const start = editor.getScrolledVisiblePosition(selection.getStartPosition());
  const end = editor.getScrolledVisiblePosition(selection.getEndPosition());
  if (!start || !end) return null;

  const layout = editor.getLayoutInfo();
  const containerWidth = layout.width;
  const containerHeight = layout.height;
  const midX = (start.left + end.left) / 2;
  const left = clampCenter(midX, containerWidth, halfWidths.toolbar);
  const previewLeft = clampCenter(midX, containerWidth, halfWidths.preview);

  const aboveTop = start.top - TOOLBAR_OFFSET_Y;
  const toolbarPlacement: "above" | "below" = aboveTop < 4 ? "below" : "above";
  const top =
    toolbarPlacement === "above"
      ? aboveTop
      : end.top + end.height + TOOLBAR_OFFSET_Y;

  const belowAnchor = end.top + end.height + TOOLBAR_OFFSET_Y;
  const aboveAnchor = start.top - TOOLBAR_OFFSET_Y;
  const spaceBelow = containerHeight - belowAnchor - EDGE_PAD;
  const spaceAbove = aboveAnchor - EDGE_PAD;
  // Prefer below the selection; flip above when there isn't room for the card.
  const previewPlacement: "above" | "below" =
    spaceBelow >= Math.min(previewHeight, 140) || spaceBelow >= spaceAbove
      ? "below"
      : "above";

  let previewTop =
    previewPlacement === "below" ? belowAnchor : aboveAnchor;
  if (previewPlacement === "below") {
    const maxTop = Math.max(EDGE_PAD, containerHeight - previewHeight - EDGE_PAD);
    previewTop = Math.min(previewTop, maxTop);
    previewTop = Math.max(EDGE_PAD, previewTop);
  } else {
    // Anchor is the bottom edge of the card (translateY(-100%)).
    const minBottom = EDGE_PAD + previewHeight;
    previewTop = Math.max(previewTop, minBottom);
    previewTop = Math.min(previewTop, containerHeight - EDGE_PAD);
  }

  return {
    top,
    left,
    previewLeft,
    previewTop,
    toolbarPlacement,
    previewPlacement,
  };
}

function selectionFromStored(
  monaco: typeof Monaco,
  stored: StoredSelection,
): Monaco.Selection {
  return new monaco.Selection(
    stored.range.startLineNumber,
    stored.range.startColumn,
    stored.range.endLineNumber,
    stored.range.endColumn,
  );
}

export function useMonacoSelectionAi({
  monaco,
  editor,
  activeTab,
  viewMode,
  providerId,
  llmReady,
  onContentChange,
  onAddToChat,
}: UseMonacoSelectionAiOptions): {
  state: SelectionAiUiState;
  overlayRef: RefObject<HTMLDivElement | null>;
  runAction: (action: SelectionAiAction, customPrompt?: string) => void;
  addToChat: () => void;
  accept: () => void;
  reject: () => void;
  retry: () => void;
  dismiss: () => void;
  setCustomPromptOpen: (open: boolean) => void;
  setMoreExpanded: (open: boolean) => void;
} {
  const [phase, setPhase] = useState<SelectionAiPhase>("hidden");
  const [position, setPosition] = useState<SelectionAiPosition | null>(null);
  const [selectedText, setSelectedText] = useState("");
  const [suggestedText, setSuggestedText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeAction, setActiveAction] = useState<SelectionAiAction | null>(null);
  const [customPromptOpen, setCustomPromptOpen] = useState(false);
  const [moreExpanded, setMoreExpanded] = useState(false);
  const [tooLong, setTooLong] = useState(false);

  const overlayRef = useRef<HTMLDivElement | null>(null);
  const storedRef = useRef<StoredSelection | null>(null);
  const lastCustomPromptRef = useRef<string | undefined>(undefined);
  const requestIdRef = useRef(0);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const holdSelectionRef = useRef(false);
  const pointerDownRef = useRef(false);
  const phaseRef = useRef<SelectionAiPhase>(phase);
  phaseRef.current = phase;
  const halfWidthsRef = useRef<HalfWidths>({
    toolbar: TOOLBAR_HALF_FALLBACK,
    preview: PREVIEW_HALF_FALLBACK,
  });
  const previewHeightRef = useRef(PREVIEW_HEIGHT_FALLBACK);

  const onContentChangeRef = useRef(onContentChange);
  onContentChangeRef.current = onContentChange;
  const onAddToChatRef = useRef(onAddToChat);
  onAddToChatRef.current = onAddToChat;
  const llmReadyRef = useRef(llmReady);
  llmReadyRef.current = llmReady;
  const providerIdRef = useRef(providerId);
  providerIdRef.current = providerId;
  const activeTabRef = useRef(activeTab);
  activeTabRef.current = activeTab;

  const dismiss = useCallback(() => {
    requestIdRef.current += 1;
    holdSelectionRef.current = false;
    storedRef.current = null;
    lastCustomPromptRef.current = undefined;
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
      debounceRef.current = null;
    }
    setPhase("hidden");
    setPosition(null);
    setSelectedText("");
    setSuggestedText(null);
    setError(null);
    setActiveAction(null);
    setCustomPromptOpen(false);
    setMoreExpanded(false);
    setTooLong(false);
  }, []);

  const syncFromSelection = useCallback(
    (selection: Monaco.Selection) => {
      if (!editor || !monaco) return;
      // While a result card (or in-flight request) is open, ignore new
      // selections — the card stays put and no second request starts.
      const currentPhase = phaseRef.current;
      if (
        currentPhase === "loading" ||
        currentPhase === "preview" ||
        currentPhase === "error"
      ) {
        return;
      }
      if (selection.isEmpty()) {
        if (!holdSelectionRef.current) dismiss();
        return;
      }
      const model = editor.getModel();
      if (!model || model.isDisposed()) {
        dismiss();
        return;
      }
      const text = model.getValueInRange(selection);
      if (!text.trim()) {
        if (!holdSelectionRef.current) dismiss();
        return;
      }
      const pos = computePosition(editor, selection, halfWidthsRef.current, previewHeightRef.current);
      if (!pos) {
        dismiss();
        return;
      }
      storedRef.current = {
        range: {
          startLineNumber: selection.startLineNumber,
          startColumn: selection.startColumn,
          endLineNumber: selection.endLineNumber,
          endColumn: selection.endColumn,
        },
        text,
      };
      holdSelectionRef.current = false;
      setSelectedText(text);
      setTooLong(text.length > SELECTION_AI_MAX_CHARS);
      setPosition(pos);
      setSuggestedText(null);
      setError(null);
      setActiveAction(null);
      setCustomPromptOpen(false);
      setMoreExpanded(false);
      setPhase("toolbar");
    },
    [dismiss, editor, monaco],
  );

  // Wait for pointer-up so the bar does not flash mid-drag, then debounce.
  useEffect(() => {
    if (!monaco || !editor || !activeTab || activeTab.kind === "image") {
      dismiss();
      return;
    }
    if (viewMode === "render") {
      dismiss();
      return;
    }

    const scheduleShow = () => {
      if (pointerDownRef.current) return;
      if (
        phaseRef.current === "loading" ||
        phaseRef.current === "preview" ||
        phaseRef.current === "error"
      ) {
        return;
      }
      if (debounceRef.current) clearTimeout(debounceRef.current);
      debounceRef.current = setTimeout(() => {
        debounceRef.current = null;
        if (pointerDownRef.current) return;
        if (
          phaseRef.current === "loading" ||
          phaseRef.current === "preview" ||
          phaseRef.current === "error"
        ) {
          return;
        }
        const selection = editor.getSelection();
        if (selection) syncFromSelection(selection);
      }, SHOW_DEBOUNCE_MS);
    };

    const onPointerDown = () => {
      pointerDownRef.current = true;
      if (debounceRef.current) {
        clearTimeout(debounceRef.current);
        debounceRef.current = null;
      }
    };

    const onPointerUp = () => {
      pointerDownRef.current = false;
      if (holdSelectionRef.current) return;
      scheduleShow();
    };

    const dom = editor.getDomNode();
    dom?.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("pointerup", onPointerUp);

    const selDisposable = editor.onDidChangeCursorSelection(() => {
      if (
        phaseRef.current === "loading" ||
        phaseRef.current === "preview" ||
        phaseRef.current === "error"
      ) {
        // Keep the result card open; ignore caret/selection changes.
        return;
      }
      const selection = editor.getSelection();
      if (!selection || selection.isEmpty()) {
        if (debounceRef.current) {
          clearTimeout(debounceRef.current);
          debounceRef.current = null;
        }
        if (!holdSelectionRef.current) dismiss();
        return;
      }
      if (holdSelectionRef.current) return;
      // Keyboard selection (Shift+arrows) has no pointerdown — still debounce.
      if (!pointerDownRef.current) scheduleShow();
    });

    const scrollDisposable = editor.onDidScrollChange(() => {
      const stored = storedRef.current;
      if (!stored || phaseRef.current === "hidden" || !monaco) return;
      // Don't follow scroll while reviewing a result — card stays put (drag ok).
      if (
        phaseRef.current === "preview" ||
        phaseRef.current === "error" ||
        phaseRef.current === "loading"
      ) {
        return;
      }
      const pos = computePosition(
        editor,
        selectionFromStored(monaco, stored),
        halfWidthsRef.current,
        previewHeightRef.current,
      );
      if (pos) setPosition(pos);
    });

    return () => {
      selDisposable.dispose();
      scrollDisposable.dispose();
      dom?.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("pointerup", onPointerUp);
      if (debounceRef.current) {
        clearTimeout(debounceRef.current);
        debounceRef.current = null;
      }
    };
  }, [monaco, editor, activeTab, viewMode, dismiss, syncFromSelection]);

  // Re-clamp after mount using real toolbar/preview size so the card stays
  // inside the editor — both horizontally (gutter) and vertically (bottom).
  useLayoutEffect(() => {
    if (phase === "hidden" || !editor || !monaco || !overlayRef.current) return;
    const stored = storedRef.current;
    if (!stored) return;
    // Don't yank a result card back after the user may have dragged it.
    if (phase === "preview" || phase === "error") return;

    const toolbarEl = overlayRef.current.querySelector(".selection-ai-toolbar");
    const previewEl = overlayRef.current.querySelector(".selection-ai-preview");
    let changed = false;
    if (toolbarEl instanceof HTMLElement && toolbarEl.offsetWidth > 0) {
      const half = toolbarEl.offsetWidth / 2;
      if (Math.abs(half - halfWidthsRef.current.toolbar) > 1) {
        halfWidthsRef.current = { ...halfWidthsRef.current, toolbar: half };
        changed = true;
      }
    }
    if (previewEl instanceof HTMLElement) {
      if (previewEl.offsetWidth > 0) {
        const half = previewEl.offsetWidth / 2;
        if (Math.abs(half - halfWidthsRef.current.preview) > 1) {
          halfWidthsRef.current = { ...halfWidthsRef.current, preview: half };
          changed = true;
        }
      }
      if (previewEl.offsetHeight > 0) {
        const height = previewEl.offsetHeight;
        if (Math.abs(height - previewHeightRef.current) > 2) {
          previewHeightRef.current = height;
          changed = true;
        }
      }
    }
    if (!changed) return;

    const pos = computePosition(
      editor,
      selectionFromStored(monaco, stored),
      halfWidthsRef.current,
      previewHeightRef.current,
    );
    if (pos) setPosition(pos);
  }, [phase, selectedText, suggestedText, customPromptOpen, moreExpanded, editor, monaco]);

  useEffect(() => {
    dismiss();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTab?.id]);

  useEffect(() => {
    if (phase === "hidden") return;

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        dismiss();
      }
    };

    const onPointerDown = (event: PointerEvent) => {
      const root = overlayRef.current;
      const target = event.target;
      if (root && target instanceof Node && root.contains(target)) {
        holdSelectionRef.current = true;
        return;
      }
      // Result / loading stays until Accept / Reject / Escape — outside
      // clicks must not dismiss it or unlock a new selection request.
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [phase, dismiss]);

  const runAction = useCallback(
    (action: SelectionAiAction, customPrompt?: string) => {
      // Block only an in-flight request; preview/error may retry from the card.
      if (phaseRef.current === "loading") return;
      const stored = storedRef.current;
      const pid = providerIdRef.current;
      if (!stored || !pid || !llmReadyRef.current) return;
      if (stored.text.length > SELECTION_AI_MAX_CHARS) return;

      holdSelectionRef.current = true;
      lastCustomPromptRef.current = customPrompt;
      setActiveAction(action);
      setCustomPromptOpen(action === "custom");
      setError(null);
      setSuggestedText(null);
      setPhase("loading");

      const requestId = ++requestIdRef.current;
      const messages = buildSelectionAiMessages(
        action,
        stored.text,
        customPrompt,
        activeTabRef.current?.path,
      );

      void llmChatOnce(pid, messages)
        .then((response) => {
          if (requestId !== requestIdRef.current) return;
          const raw = response.content ?? "";
          const cleaned = stripCodeFence(raw);
          if (!cleaned) {
            setError("Модель вернула пустой ответ");
            setPhase("error");
            return;
          }
          setSuggestedText(cleaned);
          setPhase("preview");
        })
        .catch((e) => {
          if (requestId !== requestIdRef.current) return;
          setError(toMessage(e));
          setPhase("error");
        });
    },
    [],
  );

  const accept = useCallback(() => {
    const stored = storedRef.current;
    const text = suggestedText;
    if (!editor || !stored || text === null) return;
    const model = editor.getModel();
    if (!model || model.isDisposed()) return;

    editor.executeEdits("selection-ai", [
      {
        range: stored.range,
        text,
        forceMoveMarkers: true,
      },
    ]);
    editor.pushUndoStop();
    onContentChangeRef.current(model.getValue());
    editor.focus();
    dismiss();
  }, [dismiss, editor, suggestedText]);

  const reject = useCallback(() => {
    dismiss();
  }, [dismiss]);

  // «Добавить в чат» needs neither the LLM nor the length cap — it only
  // moves text into the chat draft, no revision request is made. The
  // toolbar keeps its own button enabled accordingly.
  const addToChat = useCallback(() => {
    const stored = storedRef.current;
    if (!stored) return;
    const tab = activeTabRef.current;
    // Only project tabs carry a docs-root-relative path (see EditorTab's
    // `origin` doc comment in useEditorTabs.ts) — external tabs carry an
    // absolute OS path and virtual/plan tabs a synthetic `plan:<id>`, neither
    // of which should be shown to the user as "the file this came from".
    const filePath = tab && tab.origin === "project" ? tab.path : null;
    onAddToChatRef.current?.(stored.text, filePath);
    dismiss();
  }, [dismiss]);

  const retry = useCallback(() => {
    const action = activeAction;
    if (!action) return;
    runAction(action, lastCustomPromptRef.current);
  }, [activeAction, runAction]);

  // Toggling the collapsed row is a pure disclosure action, not a
  // commitment like typing a custom prompt — it must not freeze selection
  // tracking the way clicking inside the toolbar normally does (the global
  // pointerdown listener above sets `holdSelectionRef` on any click inside
  // the overlay, incl. this button). Without this reset, making a *new*
  // selection elsewhere after clicking «Больше» is silently ignored —
  // `onDidChangeCursorSelection` short-circuits while the ref is held — so
  // the toolbar looks stuck on the old selection with no visible way out
  // short of Escape.
  const toggleMore = useCallback((open: boolean) => {
    holdSelectionRef.current = false;
    setMoreExpanded(open);
  }, []);

  const state: SelectionAiUiState = {
    phase,
    position,
    selectedText,
    suggestedText,
    error,
    activeAction,
    customPromptOpen,
    moreExpanded,
    tooLong,
    llmReady,
  };

  return {
    state,
    overlayRef,
    runAction,
    addToChat,
    accept,
    reject,
    retry,
    dismiss,
    setCustomPromptOpen,
    setMoreExpanded: toggleMore,
  };
}
