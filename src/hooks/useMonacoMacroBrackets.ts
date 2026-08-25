import type * as Monaco from "monaco-editor";
import { useEffect } from "react";
import { shouldInsertMacroBrackets } from "../lib/asciidocMacroBrackets";
import { ASCIIDOC_LANGUAGE_ID } from "../monaco/asciidocLanguage";

/**
 * After Space or Enter on an AsciiDoc line that ends with a bare
 * `include::`/`image::`/`xref:` target, inserts `[]` before the terminator
 * so the macro is valid (`include::request.adoc[]`).
 */
export function useMonacoMacroBrackets(
  monaco: typeof Monaco | null,
  editor: Monaco.editor.IStandaloneCodeEditor | null,
) {
  useEffect(() => {
    if (!monaco || !editor) return;

    const disposable = editor.onDidChangeModelContent((event) => {
      if (event.isUndoing || event.isRedoing || event.isFlush) return;
      if (editor.getOption(monaco.editor.EditorOption.readOnly)) return;
      if (event.changes.length !== 1) return;

      const change = event.changes[0];
      if (change.rangeLength !== 0) return;
      const isNewline = change.text === "\n" || change.text === "\r\n";
      if (change.text !== " " && !isNewline) return;

      const model = editor.getModel();
      if (!model || model.isDisposed()) return;
      if (model.getLanguageId() !== ASCIIDOC_LANGUAGE_ID) return;

      const lineNumber = change.range.startLineNumber;
      const startColumn = change.range.startColumn;
      const line = model.getLineContent(lineNumber);
      const prefix = isNewline ? line : line.slice(0, startColumn - 1);
      const suffix = isNewline
        ? lineNumber < model.getLineCount()
          ? model.getLineContent(lineNumber + 1)
          : ""
        : line.slice(startColumn);

      if (!shouldInsertMacroBrackets(prefix, suffix)) return;

      editor.executeEdits("asciidoc-macro-brackets", [
        {
          range: new monaco.Range(lineNumber, startColumn, lineNumber, startColumn),
          text: "[]",
        },
      ]);
    });

    return () => disposable.dispose();
  }, [monaco, editor]);
}
