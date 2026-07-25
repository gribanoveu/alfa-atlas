/**
 * File formats the editor can open. Other formats are rejected by future
 * open-file flows; keep this list as the single source of truth for filters/UI.
 */
export const SUPPORTED_EXTENSIONS = [
  ".adoc",
  ".asciidoc",
  ".json",
  ".md",
  ".markdown",
  ".txt",
  ".puml",
  ".plantuml",
  ".yaml",
  ".yml",
  ".mmd",
  ".mermaid",
] as const;

export type SupportedExtension = (typeof SUPPORTED_EXTENSIONS)[number];

/** Human-readable labels for the welcome screen / docs. */
export const SUPPORTED_FORMAT_LABELS = [
  "AsciiDoc (.adoc, .asciidoc)",
  "JSON (.json)",
  "Markdown (.md, .markdown)",
  "Plain text (.txt)",
  "PlantUML (.puml, .plantuml)",
  "YAML (.yaml, .yml)",
  "Mermaid (.mmd, .mermaid)",
] as const;

function extensionOf(path: string): string {
  const base = path.split(/[/\\]/).pop() ?? path;
  const dot = base.lastIndexOf(".");
  if (dot <= 0) return "";
  return base.slice(dot).toLowerCase();
}

export function isSupportedFile(path: string): boolean {
  const ext = extensionOf(path);
  return (SUPPORTED_EXTENSIONS as readonly string[]).includes(ext);
}

/** Monaco language id for a supported path; unknown → plaintext. */
export function monacoLanguageFor(path: string): string {
  switch (extensionOf(path)) {
    case ".json":
      return "json";
    case ".md":
    case ".markdown":
      return "markdown";
    case ".yaml":
    case ".yml":
      return "yaml";
    case ".adoc":
    case ".asciidoc":
    case ".txt":
    case ".puml":
    case ".plantuml":
    case ".mmd":
    case ".mermaid":
    default:
      return "plaintext";
  }
}
