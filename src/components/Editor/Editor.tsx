import Editor, { type OnMount } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";
import type { editor as MonacoEditor } from "monaco-editor";
import { useCallback, useEffect, useRef, useState } from "react";
import { useMonacoCompletions } from "../../hooks/useMonacoCompletions";
import { useMonacoDiagnostics } from "../../hooks/useMonacoDiagnostics";
import { useMonacoErrorsWidget } from "../../hooks/useMonacoErrorsWidget";
import type { Diagnostic } from "../../lib/workspaceIndex";
import type { CursorPosition, EditorTab } from "../../hooks/useEditorTabs";
import type { EditorViewMode } from "../../types/viewMode";
import { AsciiDocPreview } from "../AsciiDocPreview/AsciiDocPreview";
import { PanelResizeHandle } from "../PanelResizeHandle/PanelResizeHandle";
import { EditorTabs } from "./EditorTabs";
import "./Editor.css";

type RevealRequest = {
  /** Уникальный id запроса, чтобы повторный клик по той же строке сработал. */
  id: number;
  line: number;
  column: number;
  severity: "error" | "warning";
};

type EditorPaneProps = {
  tabs: EditorTab[];
  activeTabId: string | null;
  activeTab: EditorTab | null;
  onSelectTab: (id: string) => void;
  onCloseTab: (id: string) => void;
  onCloseAllTabs: () => void;
  onCloseOtherTabs: (id: string) => void;
  onChangeContent: (content: string) => void;
  onCursorChange: (cursor: CursorPosition) => void;
  diagnostics: Diagnostic[];
  completionsEnabled: boolean;
  /** Запрос на переход к строке с диагностикой (из Problems panel). */
  revealRequest: RevealRequest | null;
  /** Открыть панель «Проблемы» (по клику на индикатор ошибок в редакторе). */
  onOpenProblems: () => void;
  /** Клик по xref-ссылке в превью AsciiDoc (path#anchor или #anchor). */
  onOpenXref?: (href: string) => void;
  viewMode: EditorViewMode;
  onViewModeChange: (mode: EditorViewMode) => void;
  docsRoot: string | null;
};

const SPLIT_INITIAL_RATIO = 0.5;
const SPLIT_MIN_RATIO = 0.15;
const SPLIT_MAX_RATIO = 0.85;

export function EditorPane({
  tabs,
  activeTabId,
  activeTab,
  onSelectTab,
  onCloseTab,
  onCloseAllTabs,
  onCloseOtherTabs,
  onChangeContent,
  onCursorChange,
  diagnostics,
  completionsEnabled,
  revealRequest,
  onOpenProblems,
  onOpenXref,
  viewMode,
  onViewModeChange,
  docsRoot,
}: EditorPaneProps) {
  const [monaco, setMonaco] = useState<typeof Monaco | null>(null);
  const [editor, setEditor] =
    useState<MonacoEditor.IStandaloneCodeEditor | null>(null);
  const highlightRef = useRef<string[]>([]);

  const handleMount: OnMount = useCallback(
    (editorInstance, monacoInstance) => {
      setMonaco(monacoInstance);
      setEditor(editorInstance);

      const syncCursor = () => {
        const position = editorInstance.getPosition();
        if (!position) return;
        onCursorChange({ line: position.lineNumber, column: position.column });
      };

      syncCursor();
      editorInstance.onDidChangeCursorPosition(syncCursor);
    },
    [onCursorChange],
  );

  useMonacoCompletions(monaco, completionsEnabled);
  useMonacoDiagnostics(monaco, editor, diagnostics, activeTab?.path ?? null);
  useMonacoErrorsWidget(
    editor,
    diagnostics,
    activeTab?.path ?? null,
    onOpenProblems,
  );

  const handleChange = useCallback(
    (value: string | undefined) => {
      onChangeContent(value ?? "");
    },
    [onChangeContent],
  );

  // Реакция на запрос «перейти к строке с ошибкой».
  useEffect(() => {
    if (!editor || !monaco || !revealRequest) return;
    const { line, column, severity } = revealRequest;
    const model = editor.getModel();
    if (!model) return;
    const lineCount = model.getLineCount();
    const targetLine = Math.max(1, Math.min(line, lineCount));

    // Снимаем предыдущую подсветку строки.
    highlightRef.current = editor.deltaDecorations(
      highlightRef.current,
      [],
    );

    // Ставим курсор и прокручиваем к строке по центру.
    editor.setPosition({
      lineNumber: targetLine,
      column: Math.max(1, column),
    });
    editor.revealLineInCenter(targetLine);
    editor.focus();

    // Подсветка строки на несколько секунд (как в IDE).
    const decorations = editor.deltaDecorations([], [
      {
        range: new monaco.Range(targetLine, 1, targetLine, 1),
        options: {
          isWholeLine: true,
          className:
            severity === "error"
              ? "df-line-highlight-error"
              : "df-line-highlight-warning",
        },
      },
    ]);
    highlightRef.current = decorations;

    // Автоматически гасим подсветку через 2.5с.
    const timer = window.setTimeout(() => {
      highlightRef.current = editor.deltaDecorations(
        highlightRef.current,
        [],
      );
    }, 2500);
    return () => window.clearTimeout(timer);
  }, [editor, monaco, revealRequest]);

  const options: MonacoEditor.IStandaloneEditorConstructionOptions = {
    fontFamily: "'JetBrains Mono', ui-monospace, monospace",
    fontSize: 13,
    minimap: { enabled: false },
    scrollBeyondLastLine: false,
    automaticLayout: true,
    padding: { top: 8 },
    renderLineHighlight: "line",
    wordWrap: "on",
    glyphMargin: true,
  };

  // Monaco монтируется единожды на активную вкладку и не размонтируется
  // при переключении режимов просмотра — так сохраняются undo-стек и
  // курсор. В режимах `render` он скрыт через `display: none`.
  const monacoNode = activeTab ? (
    <Editor
      key={activeTab.id}
      height="100%"
      theme="vs-dark"
      language={activeTab.language}
      value={activeTab.content}
      onChange={handleChange}
      onMount={handleMount}
      options={options}
    />
  ) : null;

  const previewNode = activeTab ? (
    <AsciiDocPreview
      content={activeTab.content}
      filePath={activeTab.path}
      docsRoot={docsRoot}
      monaco={monaco}
      onOpenXref={onOpenXref}
    />
  ) : null;

  return (
    <section className="editor-col">
      <EditorTabs
        tabs={tabs}
        activeTabId={activeTabId}
        onSelect={onSelectTab}
        onClose={onCloseTab}
        onCloseAll={onCloseAllTabs}
        onCloseOthers={onCloseOtherTabs}
        viewMode={viewMode}
        onViewModeChange={onViewModeChange}
      />
      <div className={`editor-body editor-body-${viewMode}`}>
        {activeTab ? (
          viewMode === "split" ? (
            <SplitLayout
              monacoNode={monacoNode}
              previewNode={previewNode}
            />
          ) : viewMode === "render" ? (
            <>
              <div className="editor-monaco-wrap" style={{ display: "none" }}>
                {monacoNode}
              </div>
              <div className="editor-preview-wrap">{previewNode}</div>
            </>
          ) : (
            <div className="editor-monaco-wrap">{monacoNode}</div>
          )
        ) : (
          <div className="editor-empty">Откройте файл в дереве документации</div>
        )}
      </div>
    </section>
  );
}

/**
 * Split-раскладка: Monaco слева, превью справа, между ними —
 * PanelResizeHandle. Пропорция хранится в EditorPane (не персистится).
 */
function SplitLayout({
  monacoNode,
  previewNode,
}: {
  monacoNode: React.ReactNode;
  previewNode: React.ReactNode;
}) {
  const [ratio, setRatio] = useState(SPLIT_INITIAL_RATIO);
  const containerRef = useRef<HTMLDivElement>(null);

  const onResize = useCallback((delta: number) => {
    const el = containerRef.current;
    if (!el) return;
    const width = el.getBoundingClientRect().width;
    if (width <= 0) return;
    setRatio((prev) => {
      const next = prev + delta / width;
      return Math.min(SPLIT_MAX_RATIO, Math.max(SPLIT_MIN_RATIO, next));
    });
  }, []);

  return (
    <div className="editor-split" ref={containerRef}>
      <div
        className="editor-split-pane editor-monaco-wrap"
        style={{ flex: `0 0 ${ratio * 100}%`, minWidth: 0 }}
      >
        {monacoNode}
      </div>
      <PanelResizeHandle
        direction="horizontal"
        onResize={onResize}
        ariaLabel="Изменить ширину панелей"
      />
      <div
        className="editor-split-pane editor-preview-wrap"
        style={{ flex: `1 1 ${1 - ratio}%`, minWidth: 0 }}
      >
        {previewNode}
      </div>
    </div>
  );
}
