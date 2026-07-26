import type * as Monaco from "monaco-editor";
import { useEffect, useRef } from "react";
import type { Diagnostic } from "../lib/workspaceIndex";

const OWNER = "workspaceIndex";

/**
 * Синхронизирует диагностики индекса с активной моделью Monaco.
 *
 * Используются только штатные API Monaco:
 *  - `monaco.editor.setModelMarkers` — волнистая подсветка (squiggly),
 *    ховер-сообщения и запись в Problems-панель самого Monaco;
 *  - `editor.deltaDecorations` с `glyphMarginClassName: "codicon codicon-error
 *    |codicon-warning"` — нативные glyph-margin иконки из шрифта codicon,
 *    который Monaco подгружает автоматически;
 *  - `overviewRuler` — нативная метка на полосе прокрутки.
 *
 * Никаких самописных SVG/`::before` — только codicon-шрифт Monaco.
 */
export function useMonacoDiagnostics(
  monaco: typeof Monaco | null,
  editor: Monaco.editor.IStandaloneCodeEditor | null,
  diagnostics: Diagnostic[],
  activePath: string | null,
) {
  // Храним id предыдущих decorations, чтобы Monaco заменил их, а не плодил.
  const decorationsRef = useRef<string[]>([]);

  useEffect(() => {
    if (!monaco || !editor || !activePath) return;
    const model = editor.getModel();
    if (!model) return;

    const forThisDoc = diagnostics.filter((d) => d.document === activePath);

    // 1. Markers — волнистая подсветка, ховер и вкладка Problems Monaco.
    const markers: Monaco.editor.IMarkerData[] = forThisDoc.map((d) => ({
      startLineNumber: d.line,
      startColumn: d.column,
      endLineNumber: d.line,
      endColumn: d.column + 1,
      message: d.message,
      severity:
        d.severity === "error"
          ? monaco.MarkerSeverity.Error
          : monaco.MarkerSeverity.Warning,
      source: "workspace-index",
    }));
    monaco.editor.setModelMarkers(model, OWNER, markers);

    // 2. Glyph-margin иконки через нативный codicon-шрифт Monaco.
    const decorations: Monaco.editor.IModelDeltaDecoration[] = forThisDoc.map(
      (d) => {
        const isError = d.severity === "error";
        return {
          range: new monaco.Range(d.line, 1, d.line, 1),
          options: {
            isWholeLine: false,
            glyphMarginClassName: isError
              ? "codicon codicon-error"
              : "codicon codicon-warning",
            glyphMarginHoverMessage: { value: d.message },
            overviewRuler: {
              color: isError ? "#ff5252" : "#ffb300",
              position: isError
                ? monaco.editor.OverviewRulerLane.Right
                : monaco.editor.OverviewRulerLane.Center,
            },
          },
        };
      },
    );

    decorationsRef.current = editor.deltaDecorations(
      decorationsRef.current,
      decorations,
    );
    // DEBUG: проверить, что decorations реально создаются.
    const modelDecs = model.getAllDecorations();
    console.log("[useMonacoDiagnostics]", {
      activePath,
      forThisDoc: forThisDoc.length,
      applied: decorations.length,
      modelDecs: modelDecs.length,
      glyphDecs: modelDecs.filter((d) => d.options.glyphMarginClassName).length,
    });

    return () => {
      monaco.editor.setModelMarkers(model, OWNER, []);
      decorationsRef.current = editor.deltaDecorations(
        decorationsRef.current,
        [],
      );
    };
  }, [monaco, editor, diagnostics, activePath]);
}
