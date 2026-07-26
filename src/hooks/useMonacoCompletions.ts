import type * as Monaco from "monaco-editor";
import { useEffect } from "react";
import type { IDisposable } from "monaco-editor";
import {
  findAnchors,
  getAttributes,
  getDocuments,
} from "../lib/workspaceIndex";

const ADOC_LANGUAGE = "plaintext";

/**
 * Derive the document id (relative path) from a Monaco model URI. The index
 * keys documents by relative path; model URIs carry that path (without a
 * leading slash) per the editor's `openFile` wiring.
 */
function documentIdFromModel(model: Monaco.editor.ITextModel): string {
  return model.uri.path.replace(/^\//, "");
}

function zeroWidthRange(position: Monaco.Position): Monaco.IRange {
  return {
    startLineNumber: position.lineNumber,
    startColumn: position.column,
    endLineNumber: position.lineNumber,
    endColumn: position.column,
  };
}

function lineUpToCursor(
  model: Monaco.editor.ITextModel,
  position: Monaco.Position,
): string {
  return model.getValueInRange({
    startLineNumber: position.lineNumber,
    startColumn: 1,
    endLineNumber: position.lineNumber,
    endColumn: position.column,
  });
}

/**
 * Registers four AsciiDoc completion providers on Monaco's `plaintext`
 * language (`.adoc` maps to plaintext in `supportedFiles.ts`) per spec
 * section 9: `include::`, `xref:`, `image::`, and `{` for attributes.
 *
 * Each provider fetches fresh data from the index on trigger — O(1) lookups
 * per spec. `provideCompletionItems` may return a Promise, so async `invoke`
 * calls work natively.
 */
export function useMonacoCompletions(
  monaco: typeof Monaco | null,
  enabled: boolean,
) {
  useEffect(() => {
    if (!monaco || !enabled) return;
    const disposers: IDisposable[] = [];

    // 1. include:: — list all documents.
    disposers.push(
      monaco.languages.registerCompletionItemProvider(ADOC_LANGUAGE, {
        triggerCharacters: [":"],
        async provideCompletionItems(model, position) {
          if (!lineUpToCursor(model, position).endsWith("include::")) {
            return null;
          }
          const docs = await getDocuments();
          const range = zeroWidthRange(position);
          return {
            suggestions: docs.map((d) => ({
              label: d.relativePath,
              kind: monaco.languages.CompletionItemKind.File,
              insertText: d.relativePath,
              range,
            })),
          };
        },
      }),
    );

    // 2. xref: — documents; after `doc#` — anchors of that doc.
    disposers.push(
      monaco.languages.registerCompletionItemProvider(ADOC_LANGUAGE, {
        triggerCharacters: [":", "#"],
        async provideCompletionItems(model, position) {
          const line = lineUpToCursor(model, position);
          const xrefIdx = line.lastIndexOf("xref:");
          if (xrefIdx === -1) return null;
          const after = line.slice(xrefIdx + "xref:".length);
          const hashIdx = after.indexOf("#");
          const range = zeroWidthRange(position);
          if (hashIdx === -1) {
            const docs = await getDocuments();
            return {
              suggestions: docs.map((d) => ({
                label: d.relativePath,
                kind: monaco.languages.CompletionItemKind.File,
                insertText: d.relativePath,
                range,
              })),
            };
          }
          const docId = after.slice(0, hashIdx);
          if (!docId) return null;
          const anchors = await findAnchors(docId);
          return {
            suggestions: anchors.map((a) => ({
              label: a.id,
              kind: monaco.languages.CompletionItemKind.Reference,
              insertText: a.id,
              range,
            })),
          };
        },
      }),
    );

    // 3. image:: — list documents (images live alongside docs in the index).
    disposers.push(
      monaco.languages.registerCompletionItemProvider(ADOC_LANGUAGE, {
        triggerCharacters: [":"],
        async provideCompletionItems(model, position) {
          if (!lineUpToCursor(model, position).endsWith("image::")) {
            return null;
          }
          const docs = await getDocuments();
          const range = zeroWidthRange(position);
          return {
            suggestions: docs.map((d) => ({
              label: d.relativePath,
              kind: monaco.languages.CompletionItemKind.File,
              insertText: d.relativePath,
              range,
            })),
          };
        },
      }),
    );

    // 4. `{` — list attributes defined in the active document.
    disposers.push(
      monaco.languages.registerCompletionItemProvider(ADOC_LANGUAGE, {
        triggerCharacters: ["{"],
        async provideCompletionItems(model, position) {
          if (!lineUpToCursor(model, position).endsWith("{")) return null;
          const docId = documentIdFromModel(model);
          const attrs = await getAttributes(docId);
          const range = zeroWidthRange(position);
          return {
            suggestions: attrs.map((a) => ({
              label: a.name,
              kind: monaco.languages.CompletionItemKind.Variable,
              insertText: a.name,
              detail: a.value || undefined,
              range,
            })),
          };
        },
      }),
    );

    return () => disposers.forEach((d) => d.dispose());
  }, [monaco, enabled]);
}
