import { invoke } from "@tauri-apps/api/core";
import type { GrepMatch } from "./aiTools";

export type DocsSearchArgs = {
  pattern: string;
  path?: string | null;
  glob?: string | null;
  caseInsensitive?: boolean | null;
  maxResults?: number | null;
};

export type DocsSearchResults = {
  matches: GrepMatch[];
  truncated: boolean;
};

/** Exact regex content search under `docsRoot` (DocsOnly). Independent of
 * the AI harness allowlist / access mode. */
export function docsSearch(
  docsRoot: string,
  args: DocsSearchArgs,
): Promise<DocsSearchResults> {
  return invoke<DocsSearchResults>("docs_search", { docsRoot, args });
}
