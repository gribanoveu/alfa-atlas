import Editor, { type OnMount } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";
import type { editor as MonacoEditor } from "monaco-editor";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useGitGutter } from "../../hooks/useGitGutter";
import { useMonacoCompletions } from "../../hooks/useMonacoCompletions";
import { useMonacoDefinitions } from "../../hooks/useMonacoDefinitions";
import { useMonacoDiagnostics } from "../../hooks/useMonacoDiagnostics";
import { useMonacoErrorsWidget } from "../../hooks/useMonacoErrorsWidget";
import { useMonacoIncludeGutter } from "../../hooks/useMonacoIncludeGutter";
import { useMonacoMacroBrackets } from "../../hooks/useMonacoMacroBrackets";
import { useMonacoOutline } from "../../hooks/useMonacoOutline";
import { useMonacoSelectionAi } from "../../hooks/useMonacoSelectionAi";
import { useMonacoSpellcheck } from "../../hooks/useMonacoSpellcheck";
import type { ConversationMode } from "../../lib/aiTools";
import {
  editorActionContextFromTab,
  resolveEditorContextActions,
  type EditorContextAction,
  type EditorContextActionDelivery,
} from "../../lib/editorContextActions";
import type { GitFileDiff } from "../../lib/git";
import type { SpellcheckConfig } from "../../lib/spellcheck";
import type { Diagnostic } from "../../lib/workspaceIndex";
import { ATLAS_DARK_THEME_ID } from "../../monaco/asciidocLanguage";
import type { CursorPosition, EditorTab } from "../../hooks/useEditorTabs";
import { planIdFromTabId } from "../../hooks/useEditorTabs";
import type { EditorViewMode } from "../../types/viewMode";
import { DocumentPreview } from "../DocumentPreview/DocumentPreview";
import { MarkdownPreview } from "../MarkdownPreview/MarkdownPreview";
import { PanelResizeHandle } from "../PanelResizeHandle/PanelResizeHandle";
import { EditorTabs, type DisplayTab } from "./EditorTabs";
import { EditorContextActionsBar } from "./EditorContextActionsBar";
import { SelectionAiPreview } from "./SelectionAiPreview";
import { SelectionAiToolbar } from "./SelectionAiToolbar";
import "./Editor.css";
import "./GitGutter.css";
import "./SelectionAi.css";

type GitGutterConfig = {
  repoRoot: string;
  docsRoot: string;
  loadFileDiff: (
    path: string,
    scope: "unstaged",
  ) => Promise<GitFileDiff | null>;
};

type RevealRequest = {
  /** Уникальный id запроса, чтобы повторный клик по той же строке сработал. */
  id: number;
  line: number;
  column: number;
  severity: "error" | "warning";
};

type InsertRequest = {
  /** Уникальный id запроса, чтобы повторная вставка того же шаблона сработала. */
  id: number;
  /** Вкладка, для которой запрошена вставка — защита от повторного срабатывания при remount. */
  tabId: string;
  text: string;
};

type EditorPaneProps = {
  tabs: DisplayTab[];
  activeTabId: string | null;
  activeTab: EditorTab | null;
  /** "file" shows the active file tab (Monaco/preview); "openapi" shows
   * `openApiExplorer` instead, regardless of `activeTab`/`activeTabId`. */
  activeKind: "file" | "openapi";
  openApiExplorer?: React.ReactNode;
  onSelectTab: (id: string) => void;
  onCloseTab: (id: string) => void;
  onCloseAllTabs: () => void;
  onCloseOtherTabs: (id: string) => void;
  onChangeContent: (content: string) => void;
  onCursorChange: (cursor: CursorPosition) => void;
  diagnostics: Diagnostic[];
  completionsEnabled: boolean;
  spellcheckConfig: SpellcheckConfig;
  /** Запрос на переход к строке с диагностикой (из Problems panel). */
  revealRequest: RevealRequest | null;
  /** Запрос на вставку AsciiDoc-шаблона в позицию курсора. */
  insertRequest: InsertRequest | null;
  /** Открыть панель «Проблемы» (по клику на индикатор ошибок в редакторе). */
  onOpenProblems: () => void;
  /** Клик по xref-ссылке в превью AsciiDoc (path#anchor или #anchor). */
  onOpenXref?: (href: string) => void;
  /** Клик по иконке перехода в жёлобе редактора рядом с include::/image::/
   * xref: (docs-relative путь + опциональный якорь) — та же функция,
   * что открывает файл по Ctrl+Click в самом Monaco. */
  onOpenDocumentReference?: (docsRelativePath: string, anchor: string | null) => void;
  viewMode: EditorViewMode;
  onViewModeChange: (mode: EditorViewMode) => void;
  docsRoot: string | null;
  gitGutter?: GitGutterConfig | null;
  editorFontSizePx: number;
  /** Active LLM provider id for selection AI actions; null when none configured. */
  providerId: string | null;
  /** True when the active provider has an API key and can serve `llmChatOnce`. */
  llmReady: boolean;
  /** «Добавить в чат» из панели выделения — передаёт выделенный текст и путь
   * файла (docs-root-relative) наверх, в App, для вставки в чат ассистента. */
  onAddToChat?: (text: string, filePath: string | null) => void;
  /** Editor context action — deliver a canned prompt to the assistant. */
  onRunContextAction?: (
    prompt: string,
    opts?: { conversationMode?: ConversationMode; delivery?: EditorContextActionDelivery },
  ) => void;
  /** Уведомляет о смене текущего экземпляра редактора (для команд Undo/Redo из меню). */
  onEditorInstanceChange?: (
    editor: MonacoEditor.IStandaloneCodeEditor | null,
  ) => void;
  /** Уведомляет о готовности monaco-неймспейса (для Ctrl+Click «перейти к файлу»). */
  onMonacoInstanceChange?: (monaco: typeof Monaco | null) => void;
};

const SPLIT_INITIAL_RATIO = 0.5;
const SPLIT_MIN_RATIO = 0.15;
const SPLIT_MAX_RATIO = 0.85;

export function EditorPane({
  tabs,
  activeTabId,
  activeTab,
  activeKind,
  openApiExplorer,
  onSelectTab,
  onCloseTab,
  onCloseAllTabs,
  onCloseOtherTabs,
  onChangeContent,
  onCursorChange,
  diagnostics,
  completionsEnabled,
  spellcheckConfig,
  revealRequest,
  insertRequest,
  onOpenProblems,
  onOpenXref,
  onOpenDocumentReference,
  viewMode,
  onViewModeChange,
  docsRoot,
  gitGutter,
  editorFontSizePx,
  providerId,
  llmReady,
  onAddToChat,
  onRunContextAction,
  onEditorInstanceChange,
  onMonacoInstanceChange,
}: EditorPaneProps) {
  const [monaco, setMonaco] = useState<typeof Monaco | null>(null);
  const [editor, setEditor] =
    useState<MonacoEditor.IStandaloneCodeEditor | null>(null);
  const highlightRef = useRef<string[]>([]);
  const lastHandledInsertIdRef = useRef(0);
  const knownTabIdsRef = useRef<Set<string>>(new Set());
  const isImageTab = activeTab?.kind === "image";
  const isPlanTab = activeTab?.kind === "plan";
  // Image tabs do not mount Monaco — do not reuse the previous file's editor
  // instance with the image path (gutter/spellcheck/diagnostics).
  // Plan tabs are virtual markdown: Monaco yes, but no git/spell/AI overlays.
  const textEditor = isImageTab ? null : editor;
  const projectTextTab = !isImageTab && !isPlanTab ? activeTab : null;

  const contextActions = useMemo(() => {
    if (activeKind !== "file" || !projectTextTab) return [];
    return resolveEditorContextActions(
      editorActionContextFromTab(projectTextTab, llmReady),
    );
  }, [activeKind, projectTextTab, llmReady]);

  const handleRunContextAction = useCallback(
    (action: EditorContextAction, inputValue?: string) => {
      if (!onRunContextAction) return;
      const prompt = action.buildPrompt(
        editorActionContextFromTab(projectTextTab!, llmReady),
        inputValue,
      );
      onRunContextAction(prompt, {
        conversationMode: action.conversationMode,
        delivery: action.delivery ?? "send",
      });
    },
    [onRunContextAction, projectTextTab, llmReady],
  );

  // Plan tabs keep their own view mode (default render) so opening a plan
  // doesn't fight the user's source/split preference for real files.
  const [planViewMode, setPlanViewMode] = useState<EditorViewMode>("render");
  useEffect(() => {
    if (isPlanTab) setPlanViewMode("render");
  }, [activeTab?.id, isPlanTab]);

  const effectiveViewMode = isPlanTab ? planViewMode : viewMode;

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

  // Уведомляем App о смене активного экземпляра редактора — так команды
  // Undo/Redo из меню «Правка» всегда бьют по текущей модели.
  useEffect(() => {
    onEditorInstanceChange?.(editor);
  }, [editor, onEditorInstanceChange]);

  useEffect(() => {
    onMonacoInstanceChange?.(monaco);
  }, [monaco, onMonacoInstanceChange]);

  // Модели теперь переживают переключение вкладок (см. `path`+`keepCurrentModel`
  // на <Editor> ниже) — значит их больше не закрывает автоматически сам
  // @monaco-editor/react. Закрываем модель ровно один раз, когда вкладка
  // реально закрыта (а не просто неактивна), иначе память утекает с каждым
  // закрытием вкладки.
  //
  // Важно: это отдельный useEffect от эффекта-«подчистки при размонтировании»
  // ниже. Если объединить их в один (с возвратом cleanup-функции из этого же
  // эффекта), React будет вызывать cleanup перед КАЖДЫМ повторным запуском
  // (при каждом изменении `tabs`), закрывая вообще все модели ещё до того,
  // как отработает сама diff-логика — а не только при реальном размонтировании.
  useEffect(() => {
    if (!monaco) return;
    const currentIds = new Set(tabs.map((tab) => tab.id));
    for (const id of knownTabIdsRef.current) {
      if (!currentIds.has(id)) {
        monaco.editor.getModel(monaco.Uri.parse(id))?.dispose();
      }
    }
    knownTabIdsRef.current = currentIds;
  }, [tabs, monaco]);

  // EditorPane размонтируется целиком (например, закрытие проекта) —
  // подчищаем всё, что ещё числится открытым. `monaco` — стабильная ссылка
  // на синглтон на всё время жизни компонента, так что этот cleanup
  // реально срабатывает только при настоящем unmount, не на каждый рендер.
  useEffect(() => {
    return () => {
      if (!monaco) return;
      for (const id of knownTabIdsRef.current) {
        monaco.editor.getModel(monaco.Uri.parse(id))?.dispose();
      }
      knownTabIdsRef.current = new Set();
    };
  }, [monaco]);

  // Если файл переименован в другое расширение уже после создания модели
  // (модель переиспользуется по `tab.id`, а не по пути), синхронизируем язык.
  useEffect(() => {
    if (!monaco || !textEditor || !activeTab || isImageTab) return;
    const model = textEditor.getModel();
    if (model && model.getLanguageId() !== activeTab.language) {
      monaco.editor.setModelLanguage(model, activeTab.language);
    }
  }, [monaco, textEditor, activeTab, isImageTab]);

  useMonacoCompletions(monaco, completionsEnabled, docsRoot, gitGutter?.repoRoot ?? null);
  useMonacoMacroBrackets(monaco, textEditor);
  useMonacoOutline(monaco);
  useMonacoDefinitions(monaco, docsRoot, gitGutter?.repoRoot ?? null);
  useMonacoDiagnostics(monaco, textEditor, diagnostics, projectTextTab?.path ?? null);
  useMonacoSpellcheck(monaco, textEditor, projectTextTab, spellcheckConfig);
  useMonacoErrorsWidget(
    textEditor,
    diagnostics,
    projectTextTab?.path ?? null,
    onOpenProblems,
  );

  useGitGutter({
    monaco,
    editor: textEditor,
    activeTab: projectTextTab,
    viewMode: isImageTab ? "render" : effectiveViewMode,
    repoRoot: gitGutter?.repoRoot ?? null,
    docsRoot: gitGutter?.docsRoot ?? docsRoot,
    loadFileDiff:
      gitGutter?.loadFileDiff ??
      (async () => null),
    onContentChange: onChangeContent,
  });

  // Регистрируется после useGitGutter — см. useMonacoIncludeGutter.ts о том,
  // почему порядок здесь важен для курсора в жёлобе.
  useMonacoIncludeGutter(
    monaco,
    editor,
    isPlanTab ? null : docsRoot,
    isPlanTab ? null : (gitGutter?.repoRoot ?? null),
    onOpenDocumentReference,
  );

  const selectionAi = useMonacoSelectionAi({
    monaco,
    editor: textEditor,
    activeTab: projectTextTab,
    viewMode: isImageTab ? "render" : effectiveViewMode,
    providerId,
    llmReady,
    onContentChange: onChangeContent,
    onAddToChat,
  });

  const selectionOverlay = (
    <div className="selection-ai-overlay" ref={selectionAi.overlayRef}>
      <SelectionAiToolbar
        state={selectionAi.state}
        onAction={selectionAi.runAction}
        onToggleCustom={selectionAi.setCustomPromptOpen}
        onAddToChat={selectionAi.addToChat}
        onToggleMore={selectionAi.setMoreExpanded}
      />
      <SelectionAiPreview
        state={selectionAi.state}
        onAccept={selectionAi.accept}
        onReject={selectionAi.reject}
        onRetry={selectionAi.retry}
      />
    </div>
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

  // Реакция на запрос «вставить AsciiDoc-шаблон в позицию курсора».
  useEffect(() => {
    if (!editor || !monaco || !insertRequest || !activeTabId) return;
    if (insertRequest.tabId !== activeTabId) return;
    if (insertRequest.id === lastHandledInsertIdRef.current) return;

    lastHandledInsertIdRef.current = insertRequest.id;

    const selection = editor.getSelection();
    const position = editor.getPosition();
    if (!position) return;

    const range =
      selection ??
      new monaco.Range(
        position.lineNumber,
        position.column,
        position.lineNumber,
        position.column,
      );

    editor.executeEdits("asciidoc-snippet", [
      {
        range,
        text: insertRequest.text,
        forceMoveMarkers: true,
      },
    ]);
    editor.pushUndoStop();
    editor.focus();
  }, [editor, monaco, insertRequest, activeTabId]);

  useEffect(() => {
    editor?.updateOptions({
      fontSize: editorFontSizePx,
      readOnly: isPlanTab,
      domReadOnly: isPlanTab,
      glyphMargin: !isPlanTab,
    });
  }, [editor, editorFontSizePx, isPlanTab]);

  const options: MonacoEditor.IStandaloneEditorConstructionOptions = {
    fontFamily: "'JetBrains Mono', ui-monospace, monospace",
    fontSize: editorFontSizePx,
    minimap: { enabled: false },
    scrollBeyondLastLine: false,
    automaticLayout: true,
    padding: { top: 8 },
    renderLineHighlight: "line",
    wordWrap: "on",
    glyphMargin: !isPlanTab,
    readOnly: isPlanTab,
    domReadOnly: isPlanTab,
    lineDecorationsWidth: 11,
    // Off so `include::`/`xref:` path completions are not drowned out by
    // document-word suggestions after mid-path edits (Monaco's default word
    // pattern splits on `/`, so path providers lose the filter fight).
    wordBasedSuggestions: "off",
    // On by default in Monaco already — set explicitly so the intent is
    // visible here rather than relying on an unstated library default.
    // Only actually pins anything for AsciiDoc, fed by useMonacoOutline's
    // DocumentSymbolProvider (sections, table headers, admonition/titled
    // blocks) — other languages fall back to indentation-based sticky lines.
    stickyScroll: { enabled: true },
  };

  // `key={activeTab.id}` всё ещё пересоздаёт сам экземпляр редактора при
  // переключении вкладок (см. большой комментарий выше про handleMount) —
  // но `path`+`keepCurrentModel` заставляют @monaco-editor/react искать уже
  // существующую модель по этому пути вместо того, чтобы каждый раз создавать
  // (и молча терять) новую. Модель, а вместе с ней стек undo/redo, курсор и
  // scroll — переживают переключение вкладок; закрывается она явно эффектом
  // выше, когда вкладка реально закрыта.
  const monacoNode = activeTab && !isImageTab ? (
    <Editor
      key={activeTab.id}
      path={activeTab.id}
      keepCurrentModel
      height="100%"
      theme={ATLAS_DARK_THEME_ID}
      language={activeTab.language}
      value={activeTab.content}
      onChange={handleChange}
      onMount={handleMount}
      options={options}
    />
  ) : null;

  const previewNode = activeTab ? (
    isPlanTab ? (
      <div className="plan-tab-preview">
        <MarkdownPreview
          content={activeTab.content}
          filePath={null}
          docsRoot={docsRoot}
          monaco={monaco}
        />
      </div>
    ) : (
      <DocumentPreview
        content={activeTab.content}
        filePath={activeTab.path}
        docsRoot={docsRoot}
        monaco={monaco}
        onOpenXref={onOpenXref}
      />
    )
  ) : null;

  const planId = isPlanTab && activeTab ? planIdFromTabId(activeTab.id) : null;
  const planActions = planId ? (
    <button
      type="button"
      className="plan-tab-start-btn"
      onClick={() => {
        window.dispatchEvent(
          new CustomEvent("atlas-start-plan", { detail: { planId } }),
        );
      }}
    >
      Начать
    </button>
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
        viewMode={effectiveViewMode}
        onViewModeChange={isPlanTab ? setPlanViewMode : onViewModeChange}
        hideViewMode={isImageTab}
        allowedViewModes={isPlanTab ? (["source", "render"] as const) : undefined}
        actions={planActions}
      />
      <div className={`editor-body editor-body-${isImageTab ? "render" : effectiveViewMode}`}>
        {activeKind === "openapi" ? (
          openApiExplorer
        ) : activeTab ? (
          isImageTab || effectiveViewMode === "render" ? (
            <>
              {monacoNode ? (
                <div className="editor-monaco-wrap" style={{ display: "none" }}>
                  {monacoNode}
                </div>
              ) : null}
              <div className="editor-preview-wrap">{previewNode}</div>
            </>
          ) : effectiveViewMode === "split" ? (
            <SplitLayout
              monacoNode={
                <>
                  {monacoNode}
                  {selectionOverlay}
                </>
              }
              previewNode={previewNode}
            />
          ) : (
            <div className="editor-monaco-wrap">
              {monacoNode}
              {isPlanTab ? null : selectionOverlay}
            </div>
          )
        ) : (
          <div className="editor-empty">Откройте файл в дереве документации</div>
        )}
      </div>
      {activeKind === "file" && contextActions.length > 0 ? (
        <EditorContextActionsBar
          actions={contextActions}
          onRunAction={handleRunContextAction}
        />
      ) : null}
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
