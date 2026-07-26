import { useEffect, useState } from "react";
import type * as Monaco from "monaco-editor";
import type { AbstractBlock } from "./types";

/**
 * Маппинг asciidoc-имён языков в Monaco language ids.
 * Большинство совпадает, но встречаются короткие алиасы.
 */
const LANGUAGE_ALIASES: Record<string, string> = {
  sh: "shell",
  bash: "shell",
  shell: "shell",
  ts: "typescript",
  tsx: "typescript",
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

/**
 * Блок кода / listing / source. asciidoctor предоставляет исходный текст
 * через `getSource()`. Подсветка — через `monaco.editor.colorize`, который
 * возвращает HTML с `<span class="mtkN">` (CSS токенов инжектируется Monaco
 * глобально при первом монтировании редактора). Если monaco недоступен или
 * язык неизвестен — fallback на моноширинный `<pre>` без подсветки.
 */
export function AscCodeBlock({
  block,
  monaco,
}: {
  block: AbstractBlock;
  monaco: typeof Monaco | null;
}) {
  const source = safeGetSource(block) ?? "";
  const rawLang = (block.getAttribute("language") as string | null)?.toLowerCase() ?? null;
  const lang = rawLang ? (LANGUAGE_ALIASES[rawLang] ?? rawLang) : null;

  const [html, setHtml] = useState<string | null>(null);

  useEffect(() => {
    if (!monaco || !lang) {
      setHtml(null);
      return;
    }
    // Guard: не вызываем colorize для незарегистрированных языков —
    // иначе Monaco вернёт неокрашенный текст (но это лишняя работа),
    // а для некоторых id может бросить.
    const known = monaco.languages.getLanguages().some((l) => l.id === lang);
    if (!known) {
      setHtml(null);
      return;
    }

    let cancelled = false;
    monaco.editor
      .colorize(source, lang, { tabSize: 2 })
      .then((out) => {
        if (!cancelled) setHtml(out);
      })
      .catch(() => {
        if (!cancelled) setHtml(null);
      });
    return () => {
      cancelled = true;
    };
  }, [source, lang, monaco]);

  return (
    <pre className="asc-code" data-lang={lang ?? rawLang ?? undefined}>
      {html ? (
        <code dangerouslySetInnerHTML={{ __html: html }} />
      ) : (
        <code>
          {source.split("\n").map((line, i) => (
            <span key={i} className="asc-code-line">
              {line || "\u00a0"}
            </span>
          ))}
        </code>
      )}
    </pre>
  );
}

function safeGetSource(block: AbstractBlock): string | null {
  const fn = (block as unknown as { getSource?: () => string }).getSource;
  return typeof fn === "function" ? fn.call(block) : null;
}
