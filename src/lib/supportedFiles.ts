/**
 * File formats the editor can open. Other formats are rejected by future
 * open-file flows; keep this list as the single source of truth for filters/UI.
 */
import { extensionOf } from "./fileExtensions";

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

/** Primary extensions offered when creating a new file (IDEA-style picker). */
export const NEW_FILE_EXTENSION_OPTIONS = [
  { ext: ".adoc", label: "AsciiDoc" },
  { ext: ".json", label: "JSON" },
  { ext: ".md", label: "Markdown" },
  { ext: ".txt", label: "Plain text" },
  { ext: ".puml", label: "PlantUML" },
  { ext: ".yaml", label: "YAML" },
  { ext: ".mmd", label: "Mermaid" },
] as const;

export const DEFAULT_NEW_FILE_EXTENSION = ".adoc" as const;

export function isSupportedFile(path: string): boolean {
  const ext = extensionOf(path);
  return (SUPPORTED_EXTENSIONS as readonly string[]).includes(ext);
}

/** True when the path is an AsciiDoc document (`.adoc` or `.asciidoc`). */
export function isAsciiDocPath(path: string): boolean {
  const ext = extensionOf(path);
  return ext === ".adoc" || ext === ".asciidoc";
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

/** Human-readable format label for the status bar. */
export function formatLabelFor(path: string): string {
  switch (extensionOf(path)) {
    case ".adoc":
    case ".asciidoc":
      return "AsciiDoc";
    case ".json":
      return "JSON";
    case ".md":
    case ".markdown":
      return "Markdown";
    case ".txt":
      return "Plain text";
    case ".puml":
    case ".plantuml":
      return "PlantUML";
    case ".yaml":
    case ".yml":
      return "YAML";
    case ".mmd":
    case ".mermaid":
      return "Mermaid";
    default:
      return "Plain text";
  }
}

/** Detect dominant line ending from file contents for the status bar. */
export function lineEndingLabelFor(content: string): string {
  const crlf = (content.match(/\r\n/g) ?? []).length;
  const crOnly = (content.match(/\r(?!\n)/g) ?? []).length;
  const lfOnly = (content.match(/(?<!\r)\n/g) ?? []).length;

  if (crlf === 0 && crOnly === 0 && lfOnly === 0) return "LF";
  if (crlf >= lfOnly && crlf >= crOnly) return "CRLF";
  if (crOnly > lfOnly) return "CR";
  return "LF";
}
