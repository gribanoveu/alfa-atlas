import type * as Monaco from "monaco-editor";
import { useEffect } from "react";
import type { Diagnostic } from "../lib/workspaceIndex";

const OWNER = "workspaceIndex";

/**
 * Converts the workspace index's diagnostics into Monaco markers and sets them
 * on the active editor model. Diagnostics are filtered to the active document
 * (matched by relative path). When the model or active path changes, the
 * previous markers are cleared and the new set is applied.
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
  useEffect(() => {
    if (!monaco || !editor || !activePath) return;
    const model = editor.getModel();
    if (!model) return;

    const markers: Monaco.editor.IMarkerData[] = diagnostics
      .filter((d) => d.document === activePath)
      .map((d) => ({
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
    return () => {
      monaco.editor.setModelMarkers(model, OWNER, []);
    };
  }, [monaco, editor, diagnostics, activePath]);
}
