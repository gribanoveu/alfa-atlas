import type * as Monaco from "monaco-editor";
import { useEffect, useRef } from "react";
import type { GitFileDiff } from "../lib/git";
import {
  applyHunkRevert,
  buildHunkDiffLines,
  computeGitGutterHunks,
  findHunkAtLine,
  hunkDecorationClass,
  renderHunkDiffHtml,
  type GitGutterHunkKind,
  type GitGutterHunk,
} from "../lib/gitGutter";
import { toRepoRelativePath } from "../lib/paths";
import type { EditorTab } from "./useEditorTabs";
import type { EditorViewMode } from "../types/viewMode";

type LoadFileDiff = (
  path: string,
  scope: "unstaged",
) => Promise<GitFileDiff | null>;

type UseGitGutterOptions = {
  monaco: typeof Monaco | null;
  editor: Monaco.editor.IStandaloneCodeEditor | null;
  activeTab: EditorTab | null;
  viewMode: EditorViewMode;
  repoRoot: string | null;
  docsRoot: string | null;
  loadFileDiff: LoadFileDiff;
  onContentChange: (content: string) => void;
};

const GUTTER_TARGET_TYPES = new Set<number>();

function ensureGutterTargets(monaco: typeof Monaco) {
  if (GUTTER_TARGET_TYPES.size > 0) return;
  GUTTER_TARGET_TYPES.add(monaco.editor.MouseTargetType.GUTTER_GLYPH_MARGIN);
  GUTTER_TARGET_TYPES.add(monaco.editor.MouseTargetType.GUTTER_LINE_NUMBERS);
  GUTTER_TARGET_TYPES.add(monaco.editor.MouseTargetType.GUTTER_LINE_DECORATIONS);
}

function buildGutterDecorations(
  monaco: typeof Monaco,
  hunks: GitGutterHunk[],
): Monaco.editor.IModelDeltaDecoration[] {
  return hunks.map((hunk) => {
    const line =
      hunk.kind === "deleted" ? Math.max(1, hunk.startLine) : hunk.startLine;
    const end = hunk.kind === "deleted" ? line : hunk.endLine;
    return {
      range: new monaco.Range(line, 1, end, 1),
      options: {
        isWholeLine: true,
        linesDecorationsClassName: hunkDecorationClass(hunk.kind),
      },
    };
  });
}

function diffEmptyHint(kind: GitGutterHunkKind): string | null {
  if (kind === "added") return "(новая строка — в репозитории не было)";
  if (kind === "deleted") return "(строка удалена)";
  return null;
}

function setDiffContent(
  el: HTMLDivElement,
  hunk: GitGutterHunk,
) {
  const lines = buildHunkDiffLines(hunk.baselineText, hunk.currentText);
  if (lines.length > 0) {
    el.innerHTML = renderHunkDiffHtml(lines);
    el.classList.remove("git-gutter-popup-diff-empty");
    return;
  }

  const hint = diffEmptyHint(hunk.kind);
  el.textContent = hint ?? "—";
  el.classList.add("git-gutter-popup-diff-empty");
}

function applyGutterDecorations(
  editor: Monaco.editor.IStandaloneCodeEditor,
  monaco: typeof Monaco,
  decorationsRef: { current: Monaco.editor.IEditorDecorationsCollection | null },
  hunks: GitGutterHunk[],
) {
  decorationsRef.current?.clear();
  if (hunks.length === 0) {
    decorationsRef.current = null;
    return;
  }
  if (editor.getModel()?.isDisposed()) return;
  decorationsRef.current = editor.createDecorationsCollection(
    buildGutterDecorations(monaco, hunks),
  );
}

function bindPopupWheelHandling(root: HTMLDivElement, diffEl: HTMLDivElement) {
  root.addEventListener(
    "wheel",
    (event) => {
      event.stopPropagation();
    },
    { capture: true, passive: true },
  );

  diffEl.addEventListener(
    "wheel",
    (event) => {
      event.stopPropagation();
      if (diffEl.scrollHeight <= diffEl.clientHeight) return;

      const prevScrollTop = diffEl.scrollTop;
      diffEl.scrollTop += event.deltaY;
      if (diffEl.scrollTop !== prevScrollTop) {
        event.preventDefault();
      }
    },
    { passive: false },
  );
}

function createPopupDom(handlers: {
  onRevert: () => void;
  onCancel: () => void;
  onPrev: () => void;
  onNext: () => void;
}) {
  const root = document.createElement("div");
  root.className = "git-gutter-popup";

  const head = document.createElement("div");
  head.className = "git-gutter-popup-head";

  const title = document.createElement("div");
  title.className = "git-gutter-popup-title";
  title.textContent = "Изменение";

  const nav = document.createElement("div");
  nav.className = "git-gutter-popup-nav";

  const prevBtn = document.createElement("button");
  prevBtn.type = "button";
  prevBtn.textContent = "←";
  prevBtn.title = "Предыдущее изменение";
  prevBtn.addEventListener("click", (event) => {
    event.stopPropagation();
    handlers.onPrev();
  });

  const nextBtn = document.createElement("button");
  nextBtn.type = "button";
  nextBtn.textContent = "→";
  nextBtn.title = "Следующее изменение";
  nextBtn.addEventListener("click", (event) => {
    event.stopPropagation();
    handlers.onNext();
  });

  nav.append(prevBtn, nextBtn);
  head.append(title, nav);

  const compareEl = document.createElement("div");
  compareEl.className = "git-gutter-popup-compare";

  const diffEl = document.createElement("div");
  diffEl.className = "git-gutter-popup-diff";

  compareEl.append(diffEl);

  const actions = document.createElement("div");
  actions.className = "git-gutter-popup-actions";

  const cancelBtn = document.createElement("button");
  cancelBtn.type = "button";
  cancelBtn.className = "git-gutter-popup-cancel";
  cancelBtn.textContent = "Закрыть";
  cancelBtn.addEventListener("click", (event) => {
    event.stopPropagation();
    handlers.onCancel();
  });

  const revertBtn = document.createElement("button");
  revertBtn.type = "button";
  revertBtn.className = "git-gutter-popup-revert";
  revertBtn.textContent = "Откатить";
  revertBtn.addEventListener("click", (event) => {
    event.stopPropagation();
    handlers.onRevert();
  });

  actions.append(cancelBtn, revertBtn);
  root.append(head, compareEl, actions);
  root.addEventListener("mousedown", (event) => event.stopPropagation());
  bindPopupWheelHandling(root, diffEl);

  return { root, diffEl, prevBtn, nextBtn };
}

class GitGutterPopupWidget implements Monaco.editor.IContentWidget {
  readonly allowEditorOverflow = true;
  private readonly domNode: HTMLDivElement;
  private readonly diffEl: HTMLDivElement;
  private readonly prevBtn: HTMLButtonElement;
  private readonly nextBtn: HTMLButtonElement;
  private lineNumber = 1;
  private readonly monaco: typeof Monaco;

  constructor(
    monaco: typeof Monaco,
    dom: ReturnType<typeof createPopupDom>,
  ) {
    this.monaco = monaco;
    this.domNode = dom.root;
    this.diffEl = dom.diffEl;
    this.prevBtn = dom.prevBtn;
    this.nextBtn = dom.nextBtn;
  }

  containsTarget(target: EventTarget | null): boolean {
    if (!(target instanceof Node)) return false;
    return this.domNode.contains(target);
  }

  getId(): string {
    return "git.gutter.popup";
  }

  getDomNode(): HTMLElement {
    return this.domNode;
  }

  getPosition(): Monaco.editor.IContentWidgetPosition | null {
    return {
      position: { lineNumber: this.lineNumber, column: 1 },
      preference: [
        this.monaco.editor.ContentWidgetPositionPreference.ABOVE,
        this.monaco.editor.ContentWidgetPositionPreference.BELOW,
      ],
    };
  }

  update(
    hunk: GitGutterHunk,
    lineNumber: number,
    nav: { hasPrev: boolean; hasNext: boolean },
  ) {
    this.lineNumber = lineNumber;
    setDiffContent(this.diffEl, hunk);
    this.prevBtn.disabled = !nav.hasPrev;
    this.nextBtn.disabled = !nav.hasNext;
  }
}

function safeRemoveContentWidget(
  editor: Monaco.editor.IStandaloneCodeEditor,
  widget: Monaco.editor.IContentWidget,
) {
  try {
    editor.removeContentWidget(widget);
  } catch {
    // Widget may already be removed if the editor remounted.
  }
}

export function useGitGutter({
  monaco,
  editor,
  activeTab,
  viewMode,
  repoRoot,
  docsRoot,
  loadFileDiff,
  onContentChange,
}: UseGitGutterOptions) {
  const baselineRef = useRef<string | null>(null);
  const hunksRef = useRef<GitGutterHunk[]>([]);
  const decorationsRef =
    useRef<Monaco.editor.IEditorDecorationsCollection | null>(null);
  const popupRef = useRef<GitGutterPopupWidget | null>(null);
  const popupVisibleRef = useRef(false);
  const activeHunkRef = useRef<GitGutterHunk | null>(null);
  const repoPathRef = useRef<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const loadFileDiffRef = useRef(loadFileDiff);
  const onContentChangeRef = useRef(onContentChange);
  loadFileDiffRef.current = loadFileDiff;
  onContentChangeRef.current = onContentChange;

  useEffect(() => {
    if (!monaco || !editor) return;

    ensureGutterTargets(monaco);

    const popupDom = createPopupDom({
      onRevert: () => {
        const hunk = activeHunkRef.current;
        const model = editor.getModel();
        if (!hunk || !model || model.isDisposed()) return;
        const next = applyHunkRevert(model.getValue(), hunk);
        editor.executeEdits("git-gutter-revert", [
          {
            range: model.getFullModelRange(),
            text: next,
            forceMoveMarkers: true,
          },
        ]);
        editor.pushUndoStop();
        onContentChangeRef.current(next);
        hidePopup();
      },
      onCancel: () => {
        hidePopup();
      },
      onPrev: () => {
        const current = activeHunkRef.current;
        const popup = popupRef.current;
        if (!current || !popup) return;
        const idx = hunksRef.current.findIndex((h) => h.id === current.id);
        if (idx <= 0) return;
        const prev = hunksRef.current[idx - 1];
        activeHunkRef.current = prev;
        popup.update(prev, prev.startLine, {
          hasPrev: idx - 1 > 0,
          hasNext: true,
        });
        editor.layoutContentWidget(popup);
        editor.revealLineInCenterIfOutsideViewport(prev.startLine);
      },
      onNext: () => {
        const current = activeHunkRef.current;
        const popup = popupRef.current;
        if (!current || !popup) return;
        const idx = hunksRef.current.findIndex((h) => h.id === current.id);
        if (idx < 0 || idx >= hunksRef.current.length - 1) return;
        const nextHunk = hunksRef.current[idx + 1];
        activeHunkRef.current = nextHunk;
        popup.update(nextHunk, nextHunk.startLine, {
          hasPrev: true,
          hasNext: idx + 1 < hunksRef.current.length - 1,
        });
        editor.layoutContentWidget(popup);
        editor.revealLineInCenterIfOutsideViewport(nextHunk.startLine);
      },
    });

    const popup = new GitGutterPopupWidget(monaco, popupDom);
    popupRef.current = popup;

    const showPopup = (hunk: GitGutterHunk, lineNumber: number) => {
      const idx = hunksRef.current.findIndex((h) => h.id === hunk.id);
      activeHunkRef.current = hunk;
      popup.update(hunk, lineNumber, {
        hasPrev: idx > 0,
        hasNext: idx >= 0 && idx < hunksRef.current.length - 1,
      });
      if (!popupVisibleRef.current) {
        editor.addContentWidget(popup);
        popupVisibleRef.current = true;
      }
      editor.layoutContentWidget(popup);
    };

    const hidePopup = () => {
      if (!popupVisibleRef.current) {
        activeHunkRef.current = null;
        return;
      }
      safeRemoveContentWidget(editor, popup);
      popupVisibleRef.current = false;
      activeHunkRef.current = null;
    };

    const onDocumentMouseDown = (event: MouseEvent) => {
      if (!popupVisibleRef.current) return;
      if (popup.containsTarget(event.target)) return;
      hidePopup();
    };

    const mouseDownDisposable = editor.onMouseDown((event) => {
      if (popup.containsTarget(event.event.target)) {
        return;
      }

      if (!GUTTER_TARGET_TYPES.has(event.target.type)) {
        return;
      }

      const line = event.target.position?.lineNumber;
      if (!line) return;

      const hunk = findHunkAtLine(hunksRef.current, line);
      if (!hunk) {
        hidePopup();
        return;
      }

      event.event.preventDefault();
      event.event.stopPropagation();
      showPopup(hunk, line);
    });

    const mouseMoveDisposable = editor.onMouseMove((event) => {
      const dom = editor.getDomNode();
      if (!dom) return;
      if (GUTTER_TARGET_TYPES.has(event.target.type)) {
        const line = event.target.position?.lineNumber;
        if (line && findHunkAtLine(hunksRef.current, line)) {
          dom.style.cursor = "pointer";
          return;
        }
      }
      dom.style.cursor = "";
    });

    const mouseLeaveDisposable = editor.onMouseLeave(() => {
      const dom = editor.getDomNode();
      if (dom) dom.style.cursor = "";
    });

    const keyDownDisposable = editor.onKeyDown((event) => {
      if (event.keyCode === monaco.KeyCode.Escape) {
        hidePopup();
      }
    });

    document.addEventListener("mousedown", onDocumentMouseDown, true);

    return () => {
      document.removeEventListener("mousedown", onDocumentMouseDown, true);
      mouseDownDisposable.dispose();
      mouseMoveDisposable.dispose();
      mouseLeaveDisposable.dispose();
      keyDownDisposable.dispose();
      hidePopup();
      const dom = editor.getDomNode();
      if (dom) dom.style.cursor = "";
      decorationsRef.current?.clear();
      decorationsRef.current = null;
      popupRef.current = null;
    };
  }, [editor, monaco]);

  useEffect(() => {
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
      debounceRef.current = null;
    }

    baselineRef.current = null;
    hunksRef.current = [];
    decorationsRef.current?.clear();
    decorationsRef.current = null;

    if (
      viewMode === "render" ||
      !monaco ||
      !editor ||
      !activeTab ||
      !repoRoot ||
      !docsRoot
    ) {
      return;
    }

    const repoPath = toRepoRelativePath(
      activeTab.path,
      repoRoot,
      docsRoot,
    );
    repoPathRef.current = repoPath;

    let cancelled = false;

    const load = async () => {
      try {
        const diff = await loadFileDiffRef.current(repoPath, "unstaged");
        if (cancelled || repoPathRef.current !== repoPath) return;
        if (!editor.getModel() || editor.getModel()?.isDisposed()) return;
        if (!diff || diff.isBinary) {
          baselineRef.current = null;
          return;
        }
        baselineRef.current = diff.original;

        const hunks = computeGitGutterHunks(diff.original, activeTab.content);
        hunksRef.current = hunks;
        applyGutterDecorations(editor, monaco, decorationsRef, hunks);
      } catch {
        baselineRef.current = null;
        hunksRef.current = [];
        decorationsRef.current?.clear();
        decorationsRef.current = null;
      }
    };

    void load();

    return () => {
      cancelled = true;
    };
  }, [activeTab, docsRoot, editor, monaco, repoRoot, viewMode]);

  useEffect(() => {
    if (
      viewMode === "render" ||
      !editor ||
      !monaco ||
      baselineRef.current === null ||
      !activeTab
    ) {
      return;
    }

    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      if (!editor.getModel() || editor.getModel()?.isDisposed()) return;
      const hunks = computeGitGutterHunks(
        baselineRef.current ?? "",
        activeTab.content,
      );
      hunksRef.current = hunks;
      applyGutterDecorations(editor, monaco, decorationsRef, hunks);
      if (
        activeHunkRef.current &&
        !hunks.some((h) => h.id === activeHunkRef.current?.id) &&
        popupRef.current &&
        popupVisibleRef.current
      ) {
        safeRemoveContentWidget(editor, popupRef.current);
        popupVisibleRef.current = false;
        activeHunkRef.current = null;
      }
    }, 150);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [activeTab?.content, activeTab, editor, monaco, viewMode]);
}
