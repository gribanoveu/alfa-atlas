import type * as Monaco from "monaco-editor";
import { useEffect } from "react";
import { findMacroTargetAt } from "../lib/asciidocReferences";
import { resolveRelativeToDocument, toDocsRelativePath, toRepoRelativePath } from "../lib/paths";
import { ASCIIDOC_LANGUAGE_ID } from "../monaco/asciidocLanguage";
import { findDocument, getDocument } from "../lib/workspaceIndex";

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
        const naiveDocsRelative = resolveRelativeToDocument(macroTarget.target, sourceDocsRelative);

        let resolvedDocsRelative: string | null = null;
        const naiveRepoRelative = toRepoRelativePath(naiveDocsRelative, repoRoot, docsRoot);
        const naiveDoc = await getDocument(naiveRepoRelative).catch(() => null);
        if (naiveDoc) {
          resolvedDocsRelative = naiveDocsRelative;
        } else {
          // Dir-relative resolution didn't hit a real document — fall back
          // to a by-filename index lookup (also covers targets that were
          // inserted as repo-relative paths by older autocomplete).
          const basename = macroTarget.target.split("/").pop() || macroTarget.target;
          const matches = await findDocument(basename).catch(() => []);
          if (matches.length > 0) {
            resolvedDocsRelative = toDocsRelativePath(matches[0].relativePath, repoRoot, docsRoot);
          }
        }
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
