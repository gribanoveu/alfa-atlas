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

export type PathSuggestionKind = "file" | "folder";

export type DocPathSuggestion = {
  kind: PathSuggestionKind;
  label: string;
  detail: string;
  insertText: string;
  /** Force Monaco to keep every pre-filtered item (see `filterText` note below). */
  filterText: string;
  sortText: string;
};

/** Minimal path entry — documents from the index or image assets on disk. */
export type PathSuggestionEntry = {
  /** Repo-relative (documents) or docs-relative (images), see `pathSpace`. */
  relativePath: string;
  fileName: string;
  docType?: DocumentType;
};

export type BuildDocPathSuggestionsArgs = {
  entries: PathSuggestionEntry[];
  sourceDocsRelative: string;
  docsRoot: string | null;
  repoRoot: string | null;
  /** In-progress path typed after the macro keyword. */
  partial: string;
  /** When omitted, every entry is eligible (images / untyped lists). */
  kinds?: readonly DocumentType[];
  excludeSelf?: boolean;
  /**
   * `repo` (default): `relativePath` is an index key; filter with
   * `isUnderDocsRoot` and convert via `toDocsRelativePath`.
   * `docs`: path is already docs-root-relative (image assets).
   */
  pathSpace?: "repo" | "docs";
};

function basenameOf(path: string): string {
  const i = path.lastIndexOf("/");
  return i === -1 ? path : path.slice(i + 1);
}

function directoryPrefix(partial: string): string {
  const i = partial.lastIndexOf("/");
  return i === -1 ? "" : partial.slice(0, i + 1);
}

function nameAfterPrefix(partial: string): string {
  const i = partial.lastIndexOf("/");
  return i === -1 ? partial : partial.slice(i + 1);
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

/** File is an immediate child of `dirPrefix` (no further `/` in the remainder). */
function isDirectChild(insertText: string, dirPrefix: string): boolean {
  const lower = insertText.toLowerCase();
  const prefix = dirPrefix.toLowerCase();
  if (prefix) {
    if (!lower.startsWith(prefix)) return false;
    const rest = insertText.slice(dirPrefix.length);
    return rest.length > 0 && !rest.includes("/");
  }
  return !insertText.includes("/");
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
 * Unique folder insertTexts one level below the current `partial` directory.
 * Leading `../` climbs are kept as part of the folder prefix
 * (`../far/` from `../far/a.adoc`), not offered as a bare `../` item.
 */
export function deriveFolderPrefixes(fileInsertTexts: string[], partial: string): string[] {
  const dirPrefix = directoryPrefix(partial);
  const namePart = nameAfterPrefix(partial).toLowerCase();
  const folders = new Map<string, string>();

  for (const path of fileInsertTexts) {
    const lower = path.toLowerCase();
    if (dirPrefix && !lower.startsWith(dirPrefix.toLowerCase())) continue;
    const rest = path.slice(dirPrefix.length);
    const parts = rest.split("/").filter((p) => p.length > 0);
    if (parts.length < 2) continue; // file at this level (or empty)

    let i = 0;
    while (i < parts.length - 1 && (parts[i] === ".." || parts[i] === ".")) {
      i += 1;
    }
    // Need a real directory segment before the filename.
    if (i >= parts.length - 1) continue;
    if (parts[i] === ".." || parts[i] === ".") continue;

    const folderParts = parts.slice(0, i + 1);
    const segmentForFilter = parts[i];
    if (namePart && !segmentForFilter.toLowerCase().startsWith(namePart)) continue;

    const folderInsert = `${dirPrefix}${folderParts.join("/")}/`;
    const key = folderInsert.toLowerCase();
    if (!folders.has(key)) folders.set(key, folderInsert);
  }

  return [...folders.values()].sort((a, b) => a.localeCompare(b));
}

/**
 * Build path completion items for `include::` / `xref:` / `image::` macros.
 *
 * Returns both files and one-level folder prefixes. Filtering is done here
 * (not by Monaco): after `/`, Monaco's word filter would otherwise empty the
 * list. Callers should set each item's Monaco `filterText` to the returned
 * `filterText` so the widget keeps the pre-filtered set.
 */
export function buildDocPathSuggestions(
  args: BuildDocPathSuggestionsArgs,
): DocPathSuggestion[] {
  const {
    entries,
    sourceDocsRelative,
    docsRoot,
    repoRoot,
    partial,
    kinds,
    excludeSelf = true,
    pathSpace = "repo",
  } = args;

  const kindSet = kinds ? new Set<DocumentType>(kinds) : null;
  const filterText = partial || "\0";
  const dirPrefix = directoryPrefix(partial);
  const browsingDir = partial.includes("/");

  const allInsertTexts: string[] = [];
  const fileCandidates: {
    insertText: string;
    fileName: string;
  }[] = [];

  for (const d of entries) {
    if (kindSet && d.docType !== undefined && !kindSet.has(d.docType)) continue;

    let docsRelative: string;
    if (pathSpace === "docs") {
      docsRelative = d.relativePath.replace(/\\/g, "/");
    } else {
      if (docsRoot && repoRoot && !isUnderDocsRoot(d.relativePath, repoRoot, docsRoot)) {
        continue;
      }
      docsRelative =
        docsRoot && repoRoot
          ? toDocsRelativePath(d.relativePath, repoRoot, docsRoot)
          : d.relativePath;
    }

    if (excludeSelf && docsRelative === sourceDocsRelative) continue;

    const insertText = relativizeToDocument(docsRelative, sourceDocsRelative);
    const fileName = d.fileName || basenameOf(insertText);
    allInsertTexts.push(insertText);
    fileCandidates.push({ insertText, fileName });
  }

  const folderInserts = deriveFolderPrefixes(allInsertTexts, partial);
  const folders: DocPathSuggestion[] = folderInserts.map((insertText) => {
    const label = insertText.replace(/\/$/, "").split("/").pop() || insertText;
    const upCount = insertText.split("/").filter((s) => s === "..").length;
    return {
      kind: "folder" as const,
      label,
      detail: insertText,
      insertText,
      filterText,
      // Folders before files: leading "0"
      sortText: `0${pad(upCount, 2)}${pad(insertText.length, 4)}${insertText}`,
    };
  });

  const scored: {
    suggestion: DocPathSuggestion;
    score: ReturnType<typeof rankScore>;
  }[] = [];

  for (const { insertText, fileName } of fileCandidates) {
    if (browsingDir) {
      if (!isDirectChild(insertText, dirPrefix)) continue;
      const namePart = nameAfterPrefix(partial).toLowerCase();
      if (namePart && !fileName.toLowerCase().startsWith(namePart)) continue;
    } else if (partial === "") {
      // Top-level files only; nested paths are reached via folders.
      if (insertText.includes("/")) continue;
    } else if (!matchesPartial(insertText, fileName, partial)) {
      continue;
    }

    const score = rankScore(insertText, fileName, partial);
    scored.push({
      score,
      suggestion: {
        kind: "file",
        label: fileName,
        detail: insertText,
        insertText,
        filterText,
        sortText: `1${pad(score.upCount, 2)}${score.basenameHit}${pad(score.length, 4)}${insertText}`,
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

  return [...folders, ...scored.map((s) => s.suggestion)];
}

/** Convenience: map index `Document[]` into builder entries. */
export function documentsToPathEntries(docs: Document[]): PathSuggestionEntry[] {
  return docs.map((d) => ({
    relativePath: d.relativePath,
    fileName: d.fileName,
    docType: d.docType,
  }));
}
