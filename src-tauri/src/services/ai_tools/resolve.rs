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
use crate::domain::ai_tools::{ToolError, ToolScope};
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
