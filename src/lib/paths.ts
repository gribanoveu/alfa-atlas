function normPath(p: string): string {
  return p.replace(/\\/g, "/").replace(/^[/\\]+/, "").replace(/[/\\]+$/, "");
}

function docsSuffix(repoRoot: string, docsRoot: string): string {
  const repo = normPath(repoRoot);
  const docs = normPath(docsRoot);
  if (!repo || !docs) return docs;
  if (docs.startsWith(repo + "/")) return docs.slice(repo.length + 1);
  return docs;
}

/**
 * Convert a repo-relative path (e.g. `src/docs/asciidoc/foo.adoc`) into a path
 * relative to docsRoot (e.g. `foo.adoc`) — which is what `editor.openFile` expects.
 */
export function toDocsRelativePath(
  documentId: string,
  repoRoot: string,
  docsRoot: string,
): string {
  if (!repoRoot || !docsRoot) return documentId;
  const suffix = docsSuffix(repoRoot, docsRoot);
  const doc = normPath(documentId);
  if (suffix && doc.startsWith(suffix + "/")) return doc.slice(suffix.length + 1);
  return documentId;
}

/** Inverse of `toDocsRelativePath`: docs-relative → repo-relative index key. */
export function toRepoRelativePath(
  docsRelativePath: string,
  repoRoot: string,
  docsRoot: string,
): string {
  if (!repoRoot || !docsRoot) return docsRelativePath;
  const suffix = docsSuffix(repoRoot, docsRoot);
  const rel = normPath(docsRelativePath);
  if (!suffix) return rel;
  return `${suffix}/${rel}`;
}

/** Collapse `.`/`..` segments in a `/`-joined relative path. */
function normalizeRelativePath(p: string): string {
  const norm = p.replace(/\\/g, "/");
  const stack: string[] = [];
  for (const segment of norm.split("/")) {
    if (segment === "" || segment === ".") continue;
    if (segment === "..") {
      stack.pop();
      continue;
    }
    stack.push(segment);
  }
  return stack.join("/") || ".";
}

/**
 * Resolve `target` (an `include::`/`image::`/`xref:` macro target, possibly
 * with a leading `./`/`../`) against the directory of
 * `sourceDocsRelativePath` — both docs-root-relative, matching what
 * `editor.openFile` expects. Collapses `.`/`..` segments.
 */
export function resolveRelativeToDocument(
  target: string,
  sourceDocsRelativePath: string,
): string {
  const baseDir = sourceDocsRelativePath.includes("/")
    ? sourceDocsRelativePath.slice(0, sourceDocsRelativePath.lastIndexOf("/"))
    : "";
  const combined = baseDir ? `${baseDir}/${target}` : target;
  return normalizeRelativePath(combined);
}

/**
 * Inverse of `resolveRelativeToDocument`: the shortest relative path from
 * the directory of `sourceDocsRelativePath` to `targetDocsRelativePath`
 * (both docs-root-relative) — used when inserting a new `include::`/
 * `image::`/`xref:` target so it's written relative to the current file,
 * matching how every existing reference in this codebase is authored.
 */
export function relativizeToDocument(
  targetDocsRelativePath: string,
  sourceDocsRelativePath: string,
): string {
  const sourceDir = sourceDocsRelativePath.includes("/")
    ? sourceDocsRelativePath.slice(0, sourceDocsRelativePath.lastIndexOf("/")).split("/")
    : [];
  const targetParts = targetDocsRelativePath.split("/").filter(Boolean);

  let common = 0;
  while (
    common < sourceDir.length &&
    common < targetParts.length &&
    sourceDir[common] === targetParts[common]
  ) {
    common += 1;
  }

  const upCount = sourceDir.length - common;
  const parts = [...Array(upCount).fill(".."), ...targetParts.slice(common)];
  return parts.join("/");
}
