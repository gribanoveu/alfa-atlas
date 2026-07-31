import type * as Monaco from "monaco-editor";
import { useEffect } from "react";
import { buildAsciidocSymbols } from "../monaco/asciidocSymbols";
import { ASCIIDOC_LANGUAGE_ID } from "../monaco/asciidocLanguage";

/**
 * Registers the `DocumentSymbol` outline provider that feeds Monaco's
 * sticky-scroll for AsciiDoc — see `asciidocSymbols.ts` for what it pins
 * (sections, table header rows, admonition/titled blocks). Monaco calls
 * `provideDocumentSymbols` itself whenever the model changes; no manual
 * content-change wiring needed here.
 */
export function useMonacoOutline(monaco: typeof Monaco | null) {
  useEffect(() => {
    if (!monaco) return;

    const disposer = monaco.languages.registerDocumentSymbolProvider(
      ASCIIDOC_LANGUAGE_ID,
      {
        provideDocumentSymbols(model) {
          return buildAsciidocSymbols(monaco, model.getValue());
        },
      },
    );

    return () => disposer.dispose();
  }, [monaco]);
}
