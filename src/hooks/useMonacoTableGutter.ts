import type * as Monaco from "monaco-editor";
import { useEffect, useRef } from "react";
import {
  findTableBlocks,
  type TableBlockRange,
} from "../lib/asciidocTableModel";

/**
 * Clickable gutter icon on the header row of each AsciiDoc `|===` table —
 * opens the visual table editor. Same decoration/click pattern as
 * `useMonacoIncludeGutter.ts`.
 */
export function useMonacoTableGutter(
  monaco: typeof Monaco | null,
  editor: Monaco.editor.IStandaloneCodeEditor | null,
  enabled: boolean,
  onEditTable: ((range: TableBlockRange) => void) | undefined,
) {
  const headerLinesRef = useRef<Map<number, TableBlockRange>>(new Map());
  const decorationsRef = useRef<string[]>([]);
  const onEditTableRef = useRef(onEditTable);
  onEditTableRef.current = onEditTable;

  useEffect(() => {
    if (!monaco || !editor || !enabled) return;

    let debounceHandle: ReturnType<typeof setTimeout> | null = null;

    const clearDecorations = () => {
      headerLinesRef.current = new Map();
      decorationsRef.current = editor.deltaDecorations(decorationsRef.current, []);
    };

    const rescan = () => {
      const model = editor.getModel();
      if (!model || model.isDisposed()) {
        clearDecorations();
        return;
      }

      const content = model.getValue();
      const blocks = findTableBlocks(content);
      const nextHeaderLines = new Map<number, TableBlockRange>(
        blocks.map((block) => [block.headerLine, block]),
      );
      headerLinesRef.current = nextHeaderLines;

      decorationsRef.current = editor.deltaDecorations(
        decorationsRef.current,
        blocks.map((block) => ({
          range: new monaco.Range(block.headerLine, 1, block.headerLine, 1),
          options: {
            isWholeLine: false,
            glyphMarginClassName:
              "codicon codicon-table asciidoc-table-gutter-icon",
            glyphMarginHoverMessage: {
              value: "Редактировать таблицу (клик по иконке)",
            },
          },
        })),
      );
    };

    const scheduleRescan = () => {
      if (debounceHandle) clearTimeout(debounceHandle);
      debounceHandle = setTimeout(rescan, 200);
    };

    scheduleRescan();
    const contentDisposable = editor.onDidChangeModelContent(scheduleRescan);
    const modelDisposable = editor.onDidChangeModel(scheduleRescan);

    const mouseDownDisposable = editor.onMouseDown((event) => {
      if (event.target.type !== monaco.editor.MouseTargetType.GUTTER_GLYPH_MARGIN) {
        return;
      }
      const line = event.target.position?.lineNumber;
      if (!line) return;
      const range = headerLinesRef.current.get(line);
      if (!range) return;

      event.event.preventDefault();
      event.event.stopPropagation();
      onEditTableRef.current?.(range);
    });

    const mouseMoveDisposable = editor.onMouseMove((event) => {
      if (event.target.type !== monaco.editor.MouseTargetType.GUTTER_GLYPH_MARGIN) {
        return;
      }
      const line = event.target.position?.lineNumber;
      if (line === undefined || !headerLinesRef.current.has(line)) return;
      const dom = editor.getDomNode();
      if (dom) dom.style.cursor = "pointer";
    });

    return () => {
      if (debounceHandle) clearTimeout(debounceHandle);
      contentDisposable.dispose();
      modelDisposable.dispose();
      mouseDownDisposable.dispose();
      mouseMoveDisposable.dispose();
      clearDecorations();
    };
  }, [monaco, editor, enabled]);
}
