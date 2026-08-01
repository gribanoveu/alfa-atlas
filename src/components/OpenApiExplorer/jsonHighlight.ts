/**
 * Basic, dependency-free JSON syntax highlighter — returns HTML with spans
 * classed to match the app's existing JSON color scheme (`.struct-*`,
 * reused from `StructuredDataPreview`'s tree view).
 *
 * Not Monaco-based: `monaco.editor.colorize(json, "json", ...)` was tried
 * first, but this monaco-editor version's JSON language support no longer
 * registers a classic Monarch tokenizer for it (its basic-languages
 * architecture moved to an LSP-driven model) — every token comes back under
 * the same generic `mtk1` class, i.e. no real highlighting. A small regex
 * tokenizer is simpler and guaranteed to work regardless of Monaco's
 * internal language registration for JSON.
 */
const TOKEN_RE =
  /("(?:\\u[0-9a-fA-F]{4}|\\[^u]|[^\\"])*"(\s*:)?|\btrue\b|\bfalse\b|\bnull\b|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g;

function escapeHtml(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

export function highlightJson(json: string): string {
  return escapeHtml(json).replace(TOKEN_RE, (match) => {
    let cls: string;
    if (match.startsWith('"')) {
      cls = /:\s*$/.test(match) ? "struct-key" : "struct-string";
    } else if (match === "true" || match === "false") {
      cls = "struct-bool";
    } else if (match === "null") {
      cls = "struct-punct";
    } else {
      cls = "struct-number";
    }
    return `<span class="${cls}">${match}</span>`;
  });
}
