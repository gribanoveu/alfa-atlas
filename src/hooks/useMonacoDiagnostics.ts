import type * as Monaco from "monaco-editor";
import { useEffect, useRef } from "react";
import type { Diagnostic } from "../lib/workspaceIndex";

const OWNER = "workspaceIndex";

/**
 * Converts the workspace index's diagnostics into Monaco markers and sets them
 * on the active editor model. Diagnostics are filtered to the active document
 * (matched by relative path). When the model or active path changes, the
 * previous markers are cleared and the new set is applied.
 *
 * Помимо маркеров, на тех же строках выставляются glyph-margin decorations
 * (иконка слева от номера строки) и overview-ruler метки справа — как в IDE,
 * чтобы пользователь сразу видел, где есть ошибки/варнинги, и мог по ним
 * ориентироваться без раскрытия панели «Проблемы».
 *
 * @param monaco Monaco namespace (null until OnMount fires).
 * @param editor Standalone code editor instance (null until OnMount fires).
 * @param diagnostics All known diagnostics from the index.
 * @param activePath Relative path of the document currently open in the editor,
 *   or null when no document is open.
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

    // 1. Markers — волнистая подсветка и ховер-сообщение.
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

    // 2. Glyph-margin + overview ruler decorations — иконки у строки.
    const overviewRulerColor = (
      severity: "error" | "warning",
    ): string =>
      severity === "error" ? "#ff5252" : "#ffb300";

    const decorations: Monaco.editor.IModelDeltaDecoration[] = forThisDoc.map(
      (d) => {
        const isError = d.severity === "error";
        return {
          range: new monaco.Range(d.line, 1, d.line, 1),
          options: {
            isWholeLine: false,
            glyphMarginClassName: isError
              ? "df-glyph-error"
              : "df-glyph-warning",
            glyphMarginHoverMessage: { value: d.message },
            overviewRuler: {
              color: overviewRulerColor(d.severity),
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

    return () => {
      monaco.editor.setModelMarkers(model, OWNER, []);
      decorationsRef.current = editor.deltaDecorations(
        decorationsRef.current,
        [],
      );
    };
  }, [monaco, editor, diagnostics, activePath]);
}
