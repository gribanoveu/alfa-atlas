import { bundledLanguages, codeToTokens, type BundledLanguage, type ThemedToken } from "shiki";

/** Maps common fenced-code language tags to Shiki bundled language ids. */
const LANGUAGE_ALIASES: Record<string, BundledLanguage> = {
  sh: "shell",
  bash: "shell",
  shell: "shell",
  ts: "typescript",
  tsx: "tsx",
  js: "javascript",
  jsx: "javascript",
  py: "python",
  rb: "ruby",
  rs: "rust",
  go: "go",
  "c++": "cpp",
  cpp: "cpp",
  c: "c",
  cs: "csharp",
  csharp: "csharp",
  java: "java",
  kt: "kotlin",
  kts: "kotlin",
  scala: "scala",
  pl: "perl",
  php: "php",
  yml: "yaml",
  yaml: "yaml",
  toml: "ini",
  ini: "ini",
  json: "json",
  xml: "xml",
  html: "html",
  css: "css",
  scss: "scss",
  sql: "sql",
  dockerfile: "dockerfile",
  docker: "dockerfile",
  make: "makefile",
  makefile: "makefile",
};

const SHIKI_THEME = "dark-plus";

const bundledLanguageIds = new Set(Object.keys(bundledLanguages));

export function resolveShikiLanguage(rawLang: string | null): BundledLanguage | null {
  if (!rawLang) return null;
  const normalized = rawLang.trim().toLowerCase();
  const aliased = LANGUAGE_ALIASES[normalized] ?? normalized;
  return bundledLanguageIds.has(aliased) ? (aliased as BundledLanguage) : null;
}

export function splitCodeLines(source: string): string[] {
  if (!source) return [""];
  return source.replace(/\n$/, "").split("\n");
}

/** Highlight `source` with Shiki. Returns `null` when the language is unknown
 * or highlighting fails — callers fall back to plain text. */
export async function highlightCodeWithShiki(
  source: string,
  rawLang: string | null,
): Promise<ThemedToken[][] | null> {
  const lang = resolveShikiLanguage(rawLang);
  if (!lang) return null;

  try {
    const { tokens } = await codeToTokens(source.replace(/\n$/, ""), {
      lang,
      theme: SHIKI_THEME,
    });
    return tokens;
  } catch {
    return null;
  }
}

export function themedTokenStyle(token: ThemedToken): Record<string, string> | undefined {
  const style: Record<string, string> = {};
  if (token.color) style.color = token.color;
  if (token.fontStyle) {
    if (token.fontStyle & 1) style.fontStyle = "italic";
    if (token.fontStyle & 2) style.fontWeight = "bold";
    if (token.fontStyle & 4) style.textDecoration = "underline";
  }
  return Object.keys(style).length > 0 ? style : undefined;
}
