//! Turning a path the model wrote into a real path on disk, and back.
//!
//! This is the enforcement point for `AiAccessMode`: every tool that takes a
//! path routes through here, and containment is resolved against
//! `scope.root` via `domain::paths` — the same primitives
//! `services::docs_fs` uses. A caller cannot widen access by passing an
//! unexpected path, only by the `ToolScope` itself having been built with a
//! wider root.

use std::path::{Path, PathBuf};

use crate::domain::ai_access::AiAccessMode;
use crate::domain::ai_tools::{DEPS_PREFIX, ToolError, ToolScope};
use crate::domain::paths;
use crate::domain::project_config::ProjectError;
use crate::services::agent_memory;

/// `ToolFileEntry::path` is always `/`-separated by construction
/// (`paths::relative_to`), so a plain `rsplit` avoids any
/// `std::path::Path`/OsStr platform quirks.
pub(super) fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Relativize `absolute` against a root when `absolute` may not exist yet
/// (write/create destinations). Prefer strip_prefix against a canonicalized
/// root — `ensure_under` already produced a path under that root — falling
/// back to `relative_to_lenient` when needed.
pub(super) fn relative_under_maybe_missing(root: &Path, absolute: &Path) -> Result<String, ToolError> {
    let root_canon = root
        .canonicalize()
        .map_err(ToolError::Io)?;
    if absolute == root_canon.as_path() {
        return Ok(".".to_string());
    }
    if let Ok(rel) = absolute.strip_prefix(&root_canon) {
        let mut parts = Vec::new();
        for component in rel.components() {
            match component {
                std::path::Component::Normal(s) => {
                    parts.push(s.to_string_lossy().into_owned());
                }
                std::path::Component::CurDir => {}
                _ => {
                    return Err(ToolError::PathEscape(absolute.display().to_string()));
                }
            }
        }
        return Ok(parts.join("/"));
    }
    Ok(paths::relative_to_lenient(root, absolute)?.replace('\\', "/"))
}

/// Resolve a mutate/`check` path against the access-mode root, then require
/// it under `docs_root`. Returns `(access_relative, docs_relative)`.
/// `docs_relative` is computed by subtracting the known docs root after
/// containment — not by stripping a prefix from the raw model argument.
///
/// When the as-is path misses (or sits outside docs), a Docs-only extra
/// prefix / Full-repo docs-relative spelling is accepted **only if that
/// alias already exists on disk** — never guessed into a new location.
pub fn resolve_mutable_docs_path(
    scope: &ToolScope,
    path: &str,
) -> Result<(String, String), ToolError> {
    match resolve_mutable_docs_path_as_given(scope, path) {
        Ok(resolved) => {
            if mutable_target_exists(scope, &resolved.1) {
                return Ok(resolved);
            }
            if let Ok(aliased) = resolve_existing_path(scope, path) {
                return access_and_docs_rel(scope, &aliased, path);
            }
            Ok(resolved)
        }
        Err(e @ ToolError::PathEscape(_)) => Err(e),
        Err(e) => match resolve_existing_path(scope, path) {
            Ok(aliased) => access_and_docs_rel(scope, &aliased, path),
            Err(_) => Err(e),
        },
    }
}

pub(super) fn mutable_target_exists(scope: &ToolScope, docs_rel: &str) -> bool {
    if docs_rel.is_empty() || docs_rel == "." {
        return scope.docs_root.exists();
    }
    paths::join_relative(&scope.docs_root, docs_rel)
        .map(|p| p.exists())
        .unwrap_or(false)
}

pub(super) fn resolve_mutable_docs_path_as_given(
    scope: &ToolScope,
    path: &str,
) -> Result<(String, String), ToolError> {
    let joined = paths::join_relative(&scope.root, path)?;
    let under_root = paths::ensure_under(&scope.root, &joined)?;
    let under_docs = match paths::ensure_under(&scope.docs_root, &under_root) {
        Ok(p) => p,
        Err(ProjectError::PathEscape(_)) => {
            return Err(ToolError::OutsideDocumentation(path.to_string()));
        }
        Err(e) => return Err(e.into()),
    };
    access_and_docs_rel(scope, &under_docs, path)
}

pub(super) fn access_and_docs_rel(
    scope: &ToolScope,
    abs: &Path,
    original: &str,
) -> Result<(String, String), ToolError> {
    let under_docs = match paths::ensure_under(&scope.docs_root, abs) {
        Ok(p) => p,
        Err(ProjectError::PathEscape(_)) => {
            return Err(ToolError::OutsideDocumentation(original.to_string()));
        }
        Err(e) => return Err(e.into()),
    };
    let access_rel = relative_under_maybe_missing(&scope.root, &under_docs)?;
    let docs_rel = relative_under_maybe_missing(&scope.docs_root, &under_docs)?;
    let access_rel = if access_rel == "." {
        String::new()
    } else {
        access_rel
    };
    let docs_rel = if docs_rel == "." {
        String::new()
    } else {
        docs_rel
    };
    Ok((access_rel, docs_rel))
}

/// Splits an `@deps/{name}/{rest}` argument into the root it names and the
/// path under it. `None` for anything that is not such an argument — an
/// ordinary repository path, `@deps` on its own (the virtual directory, which
/// names no root), or a name this project has not configured.
///
/// The returned name and root borrow from `scope`, not from `path`, so a
/// caller can build a result prefix from them without re-parsing.
pub(super) fn split_dep_path<'s>(
    scope: &'s ToolScope,
    path: &str,
) -> Option<(&'s str, &'s Path, String)> {
    let after = match path.trim_start_matches("./").strip_prefix(DEPS_PREFIX)? {
        rest if rest.starts_with('/') => rest[1..].to_string(),
        // Either `@deps` alone or something like `@depsfoo` — neither names
        // a root.
        _ => return None,
    };
    let (name, sub) = after.split_once('/').unwrap_or((after.as_str(), ""));
    let entry = scope.extra_roots().iter().find(|(n, _)| n == name)?;
    Some((entry.0.as_str(), entry.1.as_path(), sub.to_string()))
}

/// On-disk path for a model argument on a **read** tool: whatever
/// `resolve_existing_path` already resolved it to, and only when that finds
/// nothing, an `@deps/{name}/…` path resolved against its external root.
///
/// The repository is tried first so a real file can never be shadowed by
/// the virtual prefix — a repo that happens to contain a folder called
/// `@deps` keeps working exactly as before.
///
/// This being a separate entry point is the whole containment story for
/// external roots. Mutate tools call `resolve_mutable_docs_path`, which
/// reaches `resolve_existing_path` directly and never this — so no write can
/// name an external root at all, quite apart from the `docs_root`
/// containment those tools also enforce. Widening reads therefore cannot
/// widen writes by accident, including in a mutate tool written later.
///
/// Containment under the external root is enforced the same way it is
/// everywhere else: `join_relative` rejects `..` outright, `ensure_under`
/// canonicalizes and rejects anything that lands outside (a symlink out of
/// the dependency tree included).
pub(super) fn resolve_readable_path(scope: &ToolScope, path: &str) -> Result<PathBuf, ToolError> {
    match resolve_existing_path(scope, path) {
        Ok(found) => Ok(found),
        // `..` is refused outright, never reinterpreted as an external
        // path — same rule the alias fallback in `resolve_mutable_docs_path`
        // follows, and the reason this match exists rather than a plain
        // `or_else`.
        Err(e @ ToolError::PathEscape(_)) => Err(e),
        Err(e) => {
            let Some((_, root, sub)) = split_dep_path(scope, path) else {
                return Err(e);
            };
            let joined = paths::join_relative(root, &sub)?;
            let canonical = paths::ensure_under(root, &joined)?;
            if !canonical.exists() {
                return Err(ToolError::NotFound(path.to_string()));
            }
            Ok(canonical)
        }
    }
}

/// On-disk path for a model argument: as-is under `scope.root` first, then
/// the other access-mode spelling if that file/dir exists (Full-repo
/// docs-relative path, or Docs-only path that still has the docs-root
/// folder name / repo-relative prefix on it). `..` still PathEscapes
/// immediately and is never rewritten.
pub(super) fn resolve_existing_path(scope: &ToolScope, path: &str) -> Result<PathBuf, ToolError> {
    match existing_under(scope, &scope.root, path) {
        Ok(Some(p)) => return Ok(p),
        Ok(None) => {}
        Err(e @ ToolError::PathEscape(_)) => return Err(e),
        Err(_) => {}
    }
    for alias in path_aliases(scope, path) {
        if let Ok(Some(p)) = existing_under(scope, &alias.join_root, &alias.relative) {
            return Ok(p);
        }
    }
    Err(ToolError::NotFound(path.to_string()))
}

pub(super) struct PathAlias {
    join_root: PathBuf,
    relative: String,
}

pub(super) fn path_aliases(scope: &ToolScope, path: &str) -> Vec<PathAlias> {
    let mut out = Vec::new();
    match scope.mode {
        AiAccessMode::FullRepo => {
            out.push(PathAlias {
                join_root: scope.docs_root.clone(),
                relative: path.to_string(),
            });
        }
        AiAccessMode::DocsOnly => {
            for relative in stripped_docs_prefixes(scope, path) {
                out.push(PathAlias {
                    join_root: scope.root.clone(),
                    relative,
                });
            }
        }
    }
    out
}

pub(super) fn docs_root_prefixes(scope: &ToolScope) -> Vec<String> {
    let mut prefixes = Vec::new();
    if let Ok(rel) = paths::relative_to(&scope.repo_root, &scope.docs_root) {
        let rel = rel.replace('\\', "/");
        if rel != "." && !rel.is_empty() {
            prefixes.push(rel);
        }
    }
    if let Some(name) = scope.docs_root.file_name().and_then(|n| n.to_str()) {
        if !prefixes.iter().any(|p| p == name) {
            prefixes.push(name.to_string());
        }
    }
    prefixes.sort_by_key(|p| std::cmp::Reverse(p.len()));
    prefixes
}

pub(super) fn stripped_docs_prefixes(scope: &ToolScope, path: &str) -> Vec<String> {
    let path = path.trim_start_matches("./");
    let mut out = Vec::new();
    for prefix in docs_root_prefixes(scope) {
        if path == prefix {
            out.push(".".to_string());
            continue;
        }
        let with_slash = format!("{prefix}/");
        if let Some(rest) = path.strip_prefix(&with_slash) {
            if !rest.is_empty() {
                out.push(rest.to_string());
            }
        }
    }
    out
}

pub(super) fn existing_under(
    scope: &ToolScope,
    join_root: &Path,
    relative: &str,
) -> Result<Option<PathBuf>, ToolError> {
    let joined = paths::join_relative(join_root, relative)?;
    let canonical = paths::ensure_under(join_root, &joined)?;
    if !canonical.exists() {
        return Ok(None);
    }
    match paths::ensure_under(&scope.root, &canonical) {
        Ok(_) => Ok(Some(canonical)),
        Err(ProjectError::PathEscape(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Convert a repo-relative path (index/`FileId`/`DocumentId` space) into the
/// access-mode-relative path the model should see.
pub fn to_access_relative(scope: &ToolScope, repo_relative: &str) -> Option<String> {
    if repo_relative.is_empty() || repo_relative == "." {
        return Some(String::new());
    }
    let abs = scope.repo_root.join(repo_relative);
    let under_root = paths::ensure_under(&scope.root, &abs).ok()?;
    let rel = relative_under_maybe_missing(&scope.root, &under_root).ok()?;
    Some(if rel == "." { String::new() } else { rel })
}

/// Docs-root-relative → access-mode-relative (for scaffold/move side-effect
/// paths that are already docs-relative internally).
pub(super) fn docs_rel_to_access_rel(scope: &ToolScope, docs_rel: &str) -> String {
    if scope.root == scope.docs_root {
        return docs_rel.to_string();
    }
    if docs_rel.is_empty() || docs_rel == "." {
        // Access-relative path of the docs root itself under the repo.
        return relative_under_maybe_missing(&scope.root, &scope.docs_root)
            .unwrap_or_default();
    }
    let abs = scope.docs_root.join(docs_rel);
    relative_under_maybe_missing(&scope.root, &abs).unwrap_or_else(|_| docs_rel.to_string())
}

/// Resolve a scope-root-relative tool path to a repo-relative path safe for
/// `git2`, after the same `ensure_under(scope.root)` gate every other
/// read tool uses — this is what keeps `gitDiff`/`gitBlame` safe in
/// DocsOnly (they cannot read tracked blobs outside `docsRoot`).
pub(super) fn resolve_repo_relative_path(scope: &ToolScope, path: &str) -> Result<String, ToolError> {
    let joined = paths::join_relative(&scope.root, path)?;
    let canonical = paths::ensure_under(&scope.root, &joined)?;
    // Don't require the path to exist on disk — a staged-delete or
    // commit-only path may not be in the worktree, but git still knows it.
    // Containment under `scope.root` is enough.
    let rel = paths::relative_to_lenient(&scope.repo_root, &canonical)?;
    Ok(rel.replace('\\', "/"))
}

/// Hard-deny mutate tools against `{repo}/.atlas/memory/**` — the OptMem
/// store is managed only by the `memory` tool. Prompt text alone is not
/// enough when `docsRoot` is the repo root (`.txt` is a supported docs
/// extension). `relative` is docs-root-relative (after
/// `resolve_mutable_docs_path`).
pub(super) fn reject_atlas_memory_path(scope: &ToolScope, relative: &str) -> Result<(), ToolError> {
    let joined = paths::join_relative(&scope.docs_root, relative)?;
    if agent_memory::path_is_under_project_memory(&scope.repo_root, &joined) {
        return Err(ToolError::PathEscape(format!(
            "protected agent memory store (.atlas/memory): {relative}"
        )));
    }
    Ok(())
}

/// Validates an optional subdirectory argument once, shared by both mode
/// branches: returns its root-relative string form (for the docs-only
/// prefix filter) and its canonical absolute form (for the full-repo scan
/// root).
pub(super) fn resolve_subdir(
    scope: &ToolScope,
    path: Option<&str>,
) -> Result<Option<(String, PathBuf)>, ToolError> {
    let Some(path) = path else {
        return Ok(None);
    };
    if path.is_empty() || path == "." {
        return Ok(None);
    }
    let canonical = resolve_existing_path(scope, path)?;
    if !canonical.is_dir() {
        return Err(ToolError::NotFound(path.to_string()));
    }
    let rel = paths::relative_to(&scope.root, &canonical)?;
    Ok(Some((rel, canonical)))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::domain::ai_access::AiAccessMode;
    use crate::domain::ai_tools::{ToolError, ToolScope};
    use crate::services::ai_tools::testing::*;

    #[test]
    fn an_external_root_is_readable_through_the_deps_prefix() {
        let (scope, repo, dep) = scope_with_dep_root(AiAccessMode::FullRepo);

        let content = read(&scope, "@deps/acme/lib/Client.java").unwrap();

        assert!(content.contains("void send()"), "{content}");

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&dep).ok();
    }

    /// The containment gate is the same one the repository gets: `..` is
    /// refused where it is written, not resolved and then judged.
    #[test]
    fn an_external_root_cannot_be_traversed_out_of() {
        let (scope, repo, dep) = scope_with_dep_root(AiAccessMode::FullRepo);

        let err = read(&scope, "@deps/acme/../../../etc/passwd").unwrap_err();

        assert!(matches!(err, ToolError::PathEscape(_)), "{err:?}");

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&dep).ok();
    }

    /// Reading an external root never implies writing to one. This holds
    /// without any rule of its own: mutate tools resolve through
    /// `resolve_mutable_docs_path`, which additionally requires containment
    /// under `docs_root`, and never call `resolve_readable_path` at all.
    #[test]
    fn an_external_root_is_not_writable() {
        let (scope, repo, dep) = scope_with_dep_root(AiAccessMode::FullRepo);

        let err = write(&scope, "@deps/acme/lib/Client.java", "pwned").unwrap_err();

        assert!(matches!(err, ToolError::OutsideDocumentation(_)), "{err:?}");
        let untouched = fs::read_to_string(dep.join("lib/Client.java")).unwrap();
        assert!(untouched.contains("void send()"), "{untouched}");

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&dep).ok();
    }

    /// Docs-only is the documentation subtree and nothing else — a project
    /// that has external roots configured still gets none of them while that
    /// mode is active.
    #[test]
    fn external_roots_are_absent_in_docs_only() {
        let (scope, repo, dep) = scope_with_dep_root(AiAccessMode::DocsOnly);

        assert!(scope.extra_roots().is_empty());
        let err = read(&scope, "@deps/acme/lib/Client.java").unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)), "{err:?}");

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&dep).ok();
    }

    /// The prefix is virtual, so it must not shadow a real path: a
    /// repository that actually contains `@deps/acme/...` keeps resolving to
    /// its own file.
    #[test]
    fn a_real_repository_path_wins_over_the_virtual_prefix() {
        let (scope, repo, dep) = scope_with_dep_root(AiAccessMode::FullRepo);
        fs::create_dir_all(repo.join("@deps/acme/lib")).unwrap();
        fs::write(repo.join("@deps/acme/lib/Client.java"), "the repo's own\n").unwrap();

        let content = read(&scope, "@deps/acme/lib/Client.java").unwrap();

        assert_eq!(content, "the repo's own\n");

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&dep).ok();
    }

    /// A name no root is configured under is a miss, not a way to reach the
    /// filesystem.
    #[test]
    fn an_unconfigured_dep_name_resolves_to_nothing() {
        let (scope, repo, dep) = scope_with_dep_root(AiAccessMode::FullRepo);

        let err = read(&scope, "@deps/never-configured/lib/Client.java").unwrap_err();

        assert!(matches!(err, ToolError::NotFound(_)), "{err:?}");

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&dep).ok();
    }

    #[test]
    fn path_alias_does_not_invent_missing_files() {
        let (repo, docs) = fixture_repo();
        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = read(&full_repo, "updateTransactionSpecifics/missing.adoc").unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
        let err = list(&full_repo, Some("updateTransactionSpecifics")).unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
        let err = read(&docs_only, "docs/missing.adoc").unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));

        fs::remove_dir_all(&repo).ok();
    }
}
