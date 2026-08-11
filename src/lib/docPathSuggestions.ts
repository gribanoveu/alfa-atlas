import type { Document, DocumentType } from "./workspaceIndex";
import { isUnderDocsRoot, relativizeToDocument, toDocsRelativePath } from "./paths";

/** Document kinds offered after `include::`. */
export const INCLUDE_DOC_KINDS: readonly DocumentType[] = [
  "asciiDoc",
  "plantUml",
  "mermaid",
  "text",
];

/** Document kinds offered after `xref:` (path portion, before `#`). */
export const XREF_DOC_KINDS: readonly DocumentType[] = ["asciiDoc", "markdown"];

export type DocPathSuggestion = {
  label: string;
  detail: string;
  insertText: string;
  /** Force Monaco to keep every pre-filtered item (see `filterText` note below). */
  filterText: string;
  sortText: string;
};

export type BuildDocPathSuggestionsArgs = {
  docs: Document[];
  sourceDocsRelative: string;
  docsRoot: string | null;
  repoRoot: string | null;
  /** In-progress path typed after the macro keyword. */
  partial: string;
  /** When omitted, every indexed document type is eligible. */
  kinds?: readonly DocumentType[];
  excludeSelf?: boolean;
};

function basenameOf(path: string): string {
  const i = path.lastIndexOf("/");
  return i === -1 ? path : path.slice(i + 1);
}

function matchesPartial(insertText: string, fileName: string, partial: string): boolean {
  if (!partial) return true;
  const p = partial.toLowerCase();
  const path = insertText.toLowerCase();
  const name = fileName.toLowerCase();
  if (partial.includes("/")) {
    return path.startsWith(p);
  }
  return path.startsWith(p) || path.includes(p) || name.startsWith(p) || name.includes(p);
}

function rankScore(
  insertText: string,
  fileName: string,
  partial: string,
): { upCount: number; basenameHit: number; length: number } {
  const upCount = insertText.split("/").filter((s) => s === "..").length;
  const p = partial.toLowerCase();
  const name = fileName.toLowerCase();
  let basenameHit = 2;
  if (p && !partial.includes("/")) {
    if (name.startsWith(p)) basenameHit = 0;
    else if (name.includes(p)) basenameHit = 1;
  }
  return { upCount, basenameHit, length: insertText.length };
}

function pad(n: number, width: number): string {
  return String(n).padStart(width, "0");
}

/**
 * Build path completion items for `include::` / `xref:` macros.
 *
 * Filtering is done here (not by Monaco): after `/`, Monaco's word filter
 * treats the cursor as an empty word and would otherwise drop every item.
 * Callers should set each item's Monaco `filterText` to `partial` (or the
 * returned `filterText`) so the widget keeps the pre-filtered set.
 */
export function buildDocPathSuggestions(
  args: BuildDocPathSuggestionsArgs,
): DocPathSuggestion[] {
  const {
    docs,
    sourceDocsRelative,
    docsRoot,
    repoRoot,
    partial,
    kinds,
    excludeSelf = true,
  } = args;

  const kindSet = kinds ? new Set<DocumentType>(kinds) : null;
  const filterText = partial || "\0";
  const scored: { suggestion: DocPathSuggestion; score: ReturnType<typeof rankScore> }[] = [];

  for (const d of docs) {
    if (kindSet && !kindSet.has(d.docType)) continue;
    if (docsRoot && repoRoot && !isUnderDocsRoot(d.relativePath, repoRoot, docsRoot)) {
      continue;
    }

    const docsRelative =
      docsRoot && repoRoot
        ? toDocsRelativePath(d.relativePath, repoRoot, docsRoot)
        : d.relativePath;

    if (excludeSelf && docsRelative === sourceDocsRelative) continue;

    const insertText = relativizeToDocument(docsRelative, sourceDocsRelative);
    const fileName = d.fileName || basenameOf(insertText);
    if (!matchesPartial(insertText, fileName, partial)) continue;

    const score = rankScore(insertText, fileName, partial);
    scored.push({
      score,
      suggestion: {
        label: fileName,
        detail: insertText,
        insertText,
        filterText,
        sortText: `${pad(score.upCount, 2)}${score.basenameHit}${pad(score.length, 4)}${insertText}`,
      },
    });
  }

  scored.sort((a, b) => {
    if (a.score.upCount !== b.score.upCount) return a.score.upCount - b.score.upCount;
    if (a.score.basenameHit !== b.score.basenameHit) {
      return a.score.basenameHit - b.score.basenameHit;
    }
    if (a.score.length !== b.score.length) return a.score.length - b.score.length;
    return a.suggestion.insertText.localeCompare(b.suggestion.insertText);
  });

  return scored.map((s) => s.suggestion);
}
