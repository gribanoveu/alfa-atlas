import type * as Monaco from "monaco-editor";

/**
 * Monaco language id for AsciiDoc. Registered once in `monacoSetup.ts`;
 * `.adoc`/`.asciidoc` files are pointed at it via `monacoLanguageFor()` in
 * `lib/supportedFiles.ts`. Exported so other modules (completions, theme)
 * never hardcode the string.
 */
export const ASCIIDOC_LANGUAGE_ID = "asciidoc";

export const ATLAS_DARK_THEME_ID = "atlas-dark";

const asciidocLanguageConfiguration: Monaco.languages.LanguageConfiguration = {
  comments: { lineComment: "//" },
  brackets: [
    ["[", "]"],
    ["{", "}"],
    ["(", ")"],
  ],
  autoClosingPairs: [
    { open: "[", close: "]" },
    { open: "{", close: "}" },
    { open: "(", close: ")" },
    { open: "*", close: "*" },
    { open: "_", close: "_" },
    { open: "`", close: "`" },
  ],
  surroundingPairs: [
    { open: "*", close: "*" },
    { open: "_", close: "_" },
    { open: "`", close: "`" },
  ],
};

/**
 * A pragmatic Monarch grammar covering the AsciiDoc constructs this app's
 * own templates/snippets use (see `asciidocSnippets.ts`, `useMonacoCompletions.ts`):
 * headings, emphasis, listing/literal/comment blocks, tables, admonitions,
 * attributes, macros (image::/include::/xref:/link:), anchors, lists,
 * cross-references. Not a full AsciiDoc grammar (e.g. no embedded
 * source-block language highlighting, no sidebar/quote block styling) —
 * that's a lot of surface area for marginal payoff over just rendering
 * those as plain text.
 */
const asciidocMonarchLanguage: Monaco.languages.IMonarchLanguage = {
  defaultToken: "",
  tokenPostfix: ".adoc",

  tokenizer: {
    root: [
      [/^-{4,}[ \t]*$/, { token: "delimiter.adoc", next: "@fenceListing" }],
      [/^\.{4,}[ \t]*$/, { token: "delimiter.adoc", next: "@fenceLiteral" }],
      [/^\/{4,}[ \t]*$/, { token: "comment.adoc", next: "@fenceComment" }],
      [/^\|={3,}[ \t]*$/, { token: "delimiter.adoc", next: "@table" }],

      [/^={1,6}(?=\s).*$/, "adoc-heading"],
      [/^\/\/.*$/, "comment.adoc"],
      [/^:[A-Za-z][\w-]*:/, "attribute.name.adoc"],
      [/^[ \t]*\[+[^\]\n]*\]+[ \t]*$/, "annotation.adoc"],
      [/^(NOTE|TIP|IMPORTANT|WARNING|CAUTION):/, "adoc-admonition"],
      [/^[ \t]*([*-]+|\.+|\d+\.)[ \t]+/, "keyword.flow.adoc"],

      { include: "@inline" },
    ],

    // Content inside a `----` listing block (source code, commands, ...):
    // rendered as monospace-styled text, not syntax-highlighted per the
    // embedded language — that would need a per-language sub-tokenizer.
    fenceListing: [
      [/^-{4,}[ \t]*$/, { token: "delimiter.adoc", next: "@pop" }],
      [/.*$/, "string.adoc"],
    ],
    fenceLiteral: [
      [/^\.{4,}[ \t]*$/, { token: "delimiter.adoc", next: "@pop" }],
      [/.*$/, "string.adoc"],
    ],
    fenceComment: [
      [/^\/{4,}[ \t]*$/, { token: "comment.adoc", next: "@pop" }],
      [/.*$/, "comment.adoc"],
    ],
    table: [
      [/^\|={3,}[ \t]*$/, { token: "delimiter.adoc", next: "@pop" }],
      [/\|/, "delimiter.adoc"],
      // Cell content is prose (or a cell format spec like `a|`), so it goes
      // through the same bold/code/macro handling as body text — the
      // `inline` catch-all already stops at `|`, so this can't eat into the
      // next cell.
      { include: "@inline" },
    ],

    inline: [
      [/\\./, ""],

      [/`[^`\n]+`/, "string.adoc"],
      [/\+[^+\s][^+\n]*\+/, "string.adoc"],

      [/\*\*[^*\n]+\*\*/, "strong.adoc"],
      [/\*[^*\s][^*\n]*\*/, "strong.adoc"],
      [/__[^_\n]+__/, "emphasis.adoc"],
      [/_[^_\s][^_\n]*_/, "emphasis.adoc"],
      [/#[^#\s][^#\n]*#/, "emphasis.adoc"],

      [/\{[A-Za-z0-9_-]+\}/, "variable.adoc"],

      // Known macro names only (not any bare `word:`) so this can't
      // mis-fire on ordinary prose like "Note: see below" or "Type: string"
      // — real AsciiDoc macros come from a fixed, small vocabulary. Must be
      // tried before the plain-text run below: that rule would otherwise
      // consume the letters (e.g. "xref") first, leaving no leading letter
      // for this pattern to match against once it reaches the `:`.
      [
        /\b(?:image|include|video|audio|xref|link|kbd|btn|menu|icon|pass|footnote|indexterm|stem|latexmath|asciimath|mailto):{1,2}[^\s[]*/,
        "tag.adoc",
      ],
      // Bare URLs — same ordering concern as macro names above.
      [/\w+:\/\/[^\s[\]]+/, "string.adoc"],

      [/<</, "delimiter.adoc"],
      [/>>/, "delimiter.adoc"],

      [/[[\]{}()]/, "@brackets"],
      [/\+[ \t]*$/, "delimiter.adoc"],

      // Plain-text runs: cheap catch-all for anything not claimed above.
      // Listed last since it would otherwise shadow the word-initial rules
      // (macro names, URLs) by consuming their leading letters first.
      [/[^*_+#{}[\]<>|:\\\n]+/, ""],

      [/./, ""],
    ],
  },
};

export function registerAsciiDocLanguage(monaco: typeof Monaco) {
  const languages = monaco.languages.getLanguages();
  if (languages.some((lang) => lang.id === ASCIIDOC_LANGUAGE_ID)) {
    return;
  }

  monaco.languages.register({
    id: ASCIIDOC_LANGUAGE_ID,
    extensions: [".adoc", ".asciidoc"],
  });
  monaco.languages.setLanguageConfiguration(
    ASCIIDOC_LANGUAGE_ID,
    asciidocLanguageConfiguration,
  );
  monaco.languages.setMonarchTokensProvider(
    ASCIIDOC_LANGUAGE_ID,
    asciidocMonarchLanguage,
  );

  // Extends the built-in vs-dark palette (kept for every other language)
  // with a few token types this grammar introduces.
  monaco.editor.defineTheme(ATLAS_DARK_THEME_ID, {
    base: "vs-dark",
    inherit: true,
    rules: [
      { token: "adoc-heading", foreground: "4FC1FF", fontStyle: "bold" },
      { token: "adoc-admonition", foreground: "C586C0", fontStyle: "bold" },
    ],
    colors: {},
  });
}
