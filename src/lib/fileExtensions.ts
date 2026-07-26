const MARKDOWN_EXTS = new Set([".md", ".markdown"]);

/** Lowercase extension including the dot, e.g. `.adoc`. Empty if none. */
export function extensionOf(path: string): string {
  const base = path.split(/[/\\]/).pop() ?? path;
  const dot = base.lastIndexOf(".");
  if (dot <= 0) return "";
  return base.slice(dot).toLowerCase();
}

export function isMarkdownPath(path: string): boolean {
  return MARKDOWN_EXTS.has(extensionOf(path));
}

export function isJsonPath(path: string): boolean {
  return extensionOf(path) === ".json";
}

export function isYamlPath(path: string): boolean {
  const ext = extensionOf(path);
  return ext === ".yaml" || ext === ".yml";
}
