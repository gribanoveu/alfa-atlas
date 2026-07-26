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
