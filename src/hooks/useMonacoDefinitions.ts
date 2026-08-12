import type * as Monaco from "monaco-editor";
import { useEffect } from "react";
import { findMacroTargetAt, resolveMacroTargetDocsRelative } from "../lib/asciidocReferences";
import { ASCIIDOC_LANGUAGE_ID } from "../monaco/asciidocLanguage";

/**
 * Registers Ctrl+Click ("go to definition") on `include::`/`image::`/
 * `xref:` macro targets — the actual file-opening happens in a separate
 * `monaco.editor.registerEditorOpener` (registered in `App.tsx`, which is
 * where `editor.openFile`/anchor-reveal state already lives for the preview
 * pane's xref clicks); this hook only resolves *which* document a target
 * refers to.
 */
export function useMonacoDefinitions(
  monaco: typeof Monaco | null,
  docsRoot: string | null,
  repoRoot: string | null,
) {
  useEffect(() => {
    if (!monaco || !docsRoot || !repoRoot) return;

    const disposer = monaco.languages.registerDefinitionProvider(ASCIIDOC_LANGUAGE_ID, {
      async provideDefinition(model, position) {
        const lineText = model.getLineContent(position.lineNumber);
        const macroTarget = findMacroTargetAt(lineText, position.column);
        if (!macroTarget) return null;

        // Model URIs carry the docs-root-relative path without a leading
        // slash (same convention as `documentIdFromModel` in
        // useMonacoCompletions.ts).
        const sourceDocsRelative = model.uri.path.replace(/^\//, "");
        const resolvedDocsRelative = await resolveMacroTargetDocsRelative(
          macroTarget,
          sourceDocsRelative,
          repoRoot,
          docsRoot,
        );
        if (!resolvedDocsRelative) return null;

        const uri = monaco.Uri.parse(
          resolvedDocsRelative + (macroTarget.anchor ? `#${macroTarget.anchor}` : ""),
        );
        const targetRange = new monaco.Range(1, 1, 1, 1);
        const originSelectionRange = new monaco.Range(
          position.lineNumber,
          macroTarget.startColumn,
          position.lineNumber,
          macroTarget.endColumn,
        );

        return [{ uri, range: targetRange, originSelectionRange }];
      },
    });

    return () => disposer.dispose();
  }, [monaco, docsRoot, repoRoot]);
}
