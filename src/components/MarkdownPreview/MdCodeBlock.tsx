import { useEffect, useState } from "react";
import type * as Monaco from "monaco-editor";

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

/** Fenced code block in Markdown with optional Monaco syntax highlighting. */
export function MdCodeBlock({
  source,
  rawLang,
  monaco,
}: {
  source: string;
  rawLang: string | null;
  monaco: typeof Monaco | null;
}) {
  const lang = rawLang ? (LANGUAGE_ALIASES[rawLang] ?? rawLang) : null;
  const [html, setHtml] = useState<string | null>(null);

  useEffect(() => {
    if (!monaco || !lang) {
      setHtml(null);
      return;
    }
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
