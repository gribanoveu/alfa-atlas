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
 * The documentation root's path relative to the repository root (e.g.
 * `"src/docs/asciidoc"`), or `null` when the distinction doesn't matter —
 * either root is unknown, they're the same directory, or (defensively)
 * `docsRoot` doesn't actually resolve under `repoRoot`. Used to tell the
 * assistant's system prompt (`buildAssistantSystemPrompt`/
 * `buildAccessModeChangeNotice` in `assistantConfig.ts`) the real prefix a
 * Full-repo-mode `listFiles`/`readFile` path needs stripped before it can be
 * passed to a write/mutate tool, instead of leaving the model to infer it
 * from a generic illustrative example.
 *
 * Deliberately a separate function from `docsSuffix` above rather than a
 * thin wrapper around it: `docsSuffix` returns the *absolute* `docsRoot`
 * path (not `null`) whenever it isn't cleanly nested under `repoRoot` —
 * exactly the two cases this function must report as `null` (equal roots;
 * not nested) to avoid ever leaking a physical filesystem path into a
 * prompt sent to a model. `docsSuffix`'s existing callers
 * (`toDocsRelativePath`/`toRepoRelativePath`) already depend on that
 * behavior, so it isn't changed here.
 */
export function docsRootRelativeToRepo(repoRoot: string, docsRoot: string): string | null {
  const repo = normPath(repoRoot);
  const docs = normPath(docsRoot);
  if (!repo || !docs || repo === docs) return null;
  if (docs.startsWith(repo + "/")) return docs.slice(repo.length + 1);
  return null;
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

/**
 * Whether `documentRepoRelative` (a repo-relative index key, e.g.
 * `"src/docs/asciidoc/foo.adoc"`) actually falls under `docsRoot` — the
 * workspace index covers the whole repository (`WorkspaceIndex::build`
 * scans from `repoRoot`, not `docsRoot`), so it routinely holds documents
 * outside the docs tree too (READMEs, source-adjacent `.json`/`.yaml`,
 * ...). `include::`/`image::`/`xref:` targets are always resolved relative
 * to `docsRoot` and can never actually reach a file outside it (same rule
 * the assistant's own tool descriptions state), so anything outside it
 * should never be offered as a completion for one of those macros — see
 * `useMonacoCompletions.ts`. `true` when either root is missing (nothing to
 * filter against) or `docsRoot` equals `repoRoot` (every indexed document
 * already counts).
 */
export function isUnderDocsRoot(
  documentRepoRelative: string,
  repoRoot: string,
  docsRoot: string,
): boolean {
  if (!repoRoot || !docsRoot) return true;
  const suffix = docsSuffix(repoRoot, docsRoot);
  if (!suffix) return true;
  return normPath(documentRepoRelative).startsWith(suffix + "/");
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
