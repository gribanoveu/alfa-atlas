import Editor, { type OnMount } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";
import type { editor as MonacoEditor, IDisposable } from "monaco-editor";
import { useEffect, useMemo, useRef, useState, type MutableRefObject } from "react";
import type { GitConflictFile } from "../../lib/git";
import {
  buildSideText,
  collapseConflictsToPlaceholders,
  containsConflictMarkerLines,
  type ConflictBlock,
} from "../../lib/gitConflict";
import { monacoLanguageFor } from "../../lib/supportedFiles";
import { ATLAS_DARK_THEME_ID } from "../../monaco/asciidocLanguage";
import "../Welcome/CloneRepoModal.css";
import "../Git/GitFileDiffModal.css";
import "./GitConflictModal.css";

type GitConflictModalProps = {
  path: string;
  busy: boolean;
  editorFontSizePx: number;
  onClose: () => void;
  onLoadContent: (path: string) => Promise<GitConflictFile | null>;
  onResolve: (
    path: string,
    content: string,
  ) => Promise<{ ok: boolean; mergeFinished: boolean; commitHash?: string }>;
};

type IEditor = MonacoEditor.IStandaloneCodeEditor;

/**
 * A thin row anchored right where a conflict's marker lines used to be,
 * with an accept-arrow and a reject-cross at each edge — the left pair
 * faces "Ваша версия", the right pair faces "Входящая версия". Rejecting
 * one side simply accepts the other, so either vantage point resolves the
 * conflict.
 */
function createZoneNode(
  block: ConflictBlock,
  fontSizePx: number,
  onAccept: (replacement: string) => void,
): HTMLElement {
  const node = document.createElement("div");
  node.className = "conflict-zone";
  node.style.fontSize = `${fontSizePx}px`;

  const makeBtn = (symbol: string, cls: string, title: string, replacement: string) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = `conflict-edge-btn ${cls}`;
    btn.textContent = symbol;
    btn.title = title;
    // Monaco's cursor/overlay layer sits in front of the view zone's own
    // stacking context and swallows the mousedown before a click can land
    // on this button (symptom: clicking just places a text cursor) unless
    // we stop it from bubbling up to the editor here.
    btn.onmousedown = (event) => event.stopPropagation();
    btn.onclick = (event) => {
      event.stopPropagation();
      onAccept(replacement);
    };
    return btn;
  };

  const left = document.createElement("div");
  left.className = "conflict-edge-group conflict-edge-left";
  left.appendChild(
    makeBtn("→", "conflict-edge-accept", "Принять вашу версию", block.ours),
  );
  left.appendChild(
    makeBtn(
      "✗",
      "conflict-edge-reject",
      "Отклонить вашу версию (взять входящую)",
      block.theirs,
    ),
  );

  const right = document.createElement("div");
  right.className = "conflict-edge-group conflict-edge-right";
  right.appendChild(
    makeBtn(
      "✗",
      "conflict-edge-reject",
      "Отклонить входящую версию (взять вашу)",
      block.ours,
    ),
  );
  right.appendChild(
    makeBtn("←", "conflict-edge-accept", "Принять входящую версию", block.theirs),
  );

  node.appendChild(left);
  node.appendChild(right);

  return node;
}

function attachScrollSync(instance: IEditor, others: () => IEditor[]): IDisposable {
  let syncing = false;
  return instance.onDidScrollChange((e) => {
    if (syncing) return;
    syncing = true;
    const height = instance.getLayoutInfo().height;
    const maxScroll = Math.max(1, e.scrollHeight - height);
    const ratio = e.scrollTop / maxScroll;
    for (const other of others()) {
      const otherHeight = other.getLayoutInfo().height;
      const otherMax = Math.max(1, other.getScrollHeight() - otherHeight);
      other.setScrollTop(ratio * otherMax);
    }
    syncing = false;
  });
}

export function GitConflictModal({
  path,
  busy,
  editorFontSizePx,
  onClose,
  onLoadContent,
  onResolve,
}: GitConflictModalProps) {
  const [content, setContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [remaining, setRemaining] = useState(0);

  const leftRef = useRef<IEditor | null>(null);
  const centerRef = useRef<IEditor | null>(null);
  const rightRef = useRef<IEditor | null>(null);
  const monacoRef = useRef<typeof Monaco | null>(null);
  const disposablesRef = useRef<IDisposable[]>([]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setContent(null);
    setRemaining(0);

    void onLoadContent(path).then((result) => {
      if (cancelled) return;
      if (!result) {
        setError("Не удалось загрузить файл");
      } else {
        setContent(result.content);
      }
      setLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, [onLoadContent, path]);

  const actionBusy = busy || saving;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !actionBusy) onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [actionBusy, onClose]);

  const language = monacoLanguageFor(path);
  const oursSide = useMemo(() => (content !== null ? buildSideText(content, "ours") : null), [content]);
  const theirsSide = useMemo(
    () => (content !== null ? buildSideText(content, "theirs") : null),
    [content],
  );
  const collapsed = useMemo(
    () => (content !== null ? collapseConflictsToPlaceholders(content) : null),
    [content],
  );

  const readOnlyOptions = useMemo(
    () => ({
      readOnly: true,
      domReadOnly: true,
      automaticLayout: true,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      // Keep long lines horizontally scrollable in every conflict pane.
      wordWrap: "off" as const,
      fontFamily: "'JetBrains Mono', ui-monospace, monospace",
      fontSize: editorFontSizePx,
      glyphMargin: false,
      renderOverviewRuler: false,
      renderLineHighlight: "none" as const,
    }),
    [editorFontSizePx],
  );

  const centerOptions = useMemo(
    () => ({
      readOnly: false,
      automaticLayout: true,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      // Keep long lines horizontally scrollable in every conflict pane.
      wordWrap: "off" as const,
      fontFamily: "'JetBrains Mono', ui-monospace, monospace",
      fontSize: editorFontSizePx,
      glyphMargin: false,
      renderOverviewRuler: false,
    }),
    [editorFontSizePx],
  );

  const handleCenterMount: OnMount = (editorInstance, monacoInstance) => {
    centerRef.current = editorInstance;
    monacoRef.current = monacoInstance;

    const model = editorInstance.getModel();
    if (model && collapsed) {
      const total = collapsed.placeholders.length;
      setRemaining(total);

      type BlockState = { decorationId: string; zoneId: string };
      const states = new Map<number, BlockState>();

      editorInstance.changeViewZones((accessor) => {
        collapsed.placeholders.forEach((placeholder, i) => {
          const [decorationId] = editorInstance.deltaDecorations(
            [],
            [
              {
                range: new monacoInstance.Range(placeholder.line, 1, placeholder.line, 1),
                options: { isWholeLine: true, className: "conflict-placeholder-line" },
              },
            ],
          );

          const resolve = (replacement: string) => {
            const state = states.get(i);
            if (!state) return;
            const range = model.getDecorationRange(state.decorationId);
            if (!range) return;

            const lineCount = model.getLineCount();
            const isLastLine = range.endLineNumber >= lineCount;
            const editRange = isLastLine
              ? new monacoInstance.Range(
                  range.startLineNumber,
                  1,
                  range.endLineNumber,
                  model.getLineMaxColumn(range.endLineNumber),
                )
              : new monacoInstance.Range(range.startLineNumber, 1, range.endLineNumber + 1, 1);
            const text = isLastLine || replacement.length === 0 ? replacement : `${replacement}\n`;
            editorInstance.executeEdits("conflict-resolve", [{ range: editRange, text }]);

            editorInstance.changeViewZones((acc) => acc.removeZone(state.zoneId));
            editorInstance.deltaDecorations([state.decorationId], []);
            states.delete(i);
            setRemaining((r) => r - 1);
            setError(null);
          };

          const domNode = createZoneNode(placeholder.block, editorFontSizePx, resolve);
          const zoneId = accessor.addZone({
            afterLineNumber: placeholder.line,
            heightInPx: Math.max(24, editorFontSizePx * 1.9),
            domNode,
          });

          states.set(i, { decorationId, zoneId });
        });
      });
    }

    disposablesRef.current.push(
      attachScrollSync(editorInstance, () =>
        [leftRef.current, rightRef.current].filter((e): e is IEditor => e !== null),
      ),
    );
    editorInstance.onDidDispose(() => {
      disposablesRef.current.forEach((d) => d.dispose());
      disposablesRef.current = [];
    });
  };

  const handleSideMount = (
    ref: MutableRefObject<IEditor | null>,
    ranges: { startLine: number; endLine: number }[],
    decorationClass: string,
  ): OnMount => {
    return (editorInstance, monacoInstance) => {
      ref.current = editorInstance;
      editorInstance.deltaDecorations(
        [],
        ranges.map((r) => ({
          range: new monacoInstance.Range(r.startLine, 1, r.endLine, 1),
          options: { isWholeLine: true, className: decorationClass },
        })),
      );
      disposablesRef.current.push(
        attachScrollSync(editorInstance, () =>
          [centerRef.current, ref === leftRef ? rightRef.current : leftRef.current].filter(
            (e): e is IEditor => e !== null,
          ),
        ),
      );
    };
  };

  const handleSave = async () => {
    const editorInstance = centerRef.current;
    if (!editorInstance) return;
    const value = editorInstance.getValue();

    if (containsConflictMarkerLines(value)) {
      setError(
        "В тексте остались служебные строки git (начинающиеся с <<<<<<<, ======= или >>>>>>>) — уберите их перед сохранением",
      );
      return;
    }

    setSaving(true);
    setError(null);
    try {
      const result = await onResolve(path, value);
      if (!result.ok) {
        setError("Не удалось сохранить разрешение конфликта");
        return;
      }
      onClose();
    } finally {
      setSaving(false);
    }
  };

  const canSave = !loading && content !== null && remaining === 0 && !actionBusy;

  return (
    <div
      className="clone-modal-backdrop git-diff-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (!actionBusy && event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="clone-modal git-diff-modal git-conflict-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="git-conflict-title"
      >
        <div className="git-diff-head">
          <div>
            <div className="clone-modal-title" id="git-conflict-title">
              {path}
            </div>
            <div className="git-diff-meta">
              <span className="git-status git-status-U">U</span>
              <span
                className={`git-conflict-counter${remaining === 0 ? " git-conflict-counter-clear" : ""}`}
              >
                {remaining === 0
                  ? "Все конфликты разрешены"
                  : `Осталось конфликтов: ${remaining}`}
              </span>
              <span className="git-diff-hint">
                Слева — ваша версия, справа — входящая, по центру — результат
              </span>
            </div>
          </div>
          <button
            type="button"
            className="git-diff-close"
            aria-label="Закрыть"
            disabled={actionBusy}
            onClick={onClose}
          >
            ×
          </button>
        </div>

        <div className="git-diff-body git-conflict-body">
          {loading ? (
            <div className="git-diff-placeholder">Загрузка файла…</div>
          ) : error && content === null ? (
            <div className="git-diff-placeholder git-diff-error">{error}</div>
          ) : content !== null && oursSide && theirsSide && collapsed ? (
            <div className="conflict-merge-grid">
              <div className="conflict-merge-pane">
                <div className="conflict-merge-pane-head conflict-pane-ours">
                  Ваша версия
                </div>
                <div className="conflict-merge-pane-editor">
                  <Editor
                    theme={ATLAS_DARK_THEME_ID}
                    language={language}
                    value={oursSide.text}
                    onMount={handleSideMount(leftRef, oursSide.ranges, "conflict-range-ours")}
                    options={readOnlyOptions}
                  />
                </div>
              </div>
              <div className="conflict-merge-pane conflict-merge-pane-result">
                <div className="conflict-merge-pane-head conflict-pane-result">Результат</div>
                <div className="conflict-merge-pane-editor">
                  <Editor
                    theme={ATLAS_DARK_THEME_ID}
                    language={language}
                    defaultValue={collapsed.text}
                    onMount={handleCenterMount}
                    options={centerOptions}
                  />
                </div>
              </div>
              <div className="conflict-merge-pane">
                <div className="conflict-merge-pane-head conflict-pane-theirs">
                  Входящая версия
                </div>
                <div className="conflict-merge-pane-editor">
                  <Editor
                    theme={ATLAS_DARK_THEME_ID}
                    language={language}
                    value={theirsSide.text}
                    onMount={handleSideMount(rightRef, theirsSide.ranges, "conflict-range-theirs")}
                    options={readOnlyOptions}
                  />
                </div>
              </div>
            </div>
          ) : null}
        </div>

        <div className="clone-modal-actions git-diff-actions">
          <span
            className={
              error && content !== null
                ? "git-conflict-hint-footer git-diff-error"
                : "git-conflict-hint-footer"
            }
          >
            {error && content !== null
              ? error
              : "Нажмите «Взять вашу версию» / «Взять входящую» в панели результата для каждого конфликта"}
          </span>
          <div className="git-diff-actions-right">
            <button
              type="button"
              className="clone-modal-btn"
              disabled={actionBusy}
              onClick={onClose}
            >
              Закрыть
            </button>
            <button
              type="button"
              className="clone-modal-btn primary"
              disabled={!canSave}
              onClick={() => void handleSave()}
              title={
                remaining > 0
                  ? "Сначала разрешите все конфликты в панели результата"
                  : "Сохранить разрешение и продолжить"
              }
            >
              {saving ? "Сохранение…" : "Разрешить конфликт"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
