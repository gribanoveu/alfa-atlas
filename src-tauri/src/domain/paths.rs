use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use super::project_config::ProjectError;

/// Canonicalize `path` and ensure it is equal to or under `root`.
pub fn ensure_under(root: &Path, path: &Path) -> Result<PathBuf, ProjectError> {
    let root = root
        .canonicalize()
        .map_err(ProjectError::Canonicalize)?;
    let canonical = if path.exists() {
        path.canonicalize().map_err(ProjectError::Canonicalize)?
    } else {
        // For a not-yet-existing target (a write/create destination, or
        // simply a `readFile`/`listFiles` path the caller got wrong): walk
        // up to the nearest ancestor that *does* exist — not just the
        // immediate parent — and rejoin the missing tail onto its
        // canonical form. A target two or more directories deep in a tree
        // that doesn't exist yet is the normal case for `writeFile`'s
        // documented "missing parent directories are created
        // automatically" behavior (the actual `fs::create_dir_all` for
        // those directories happens later, in `docs_fs::write_project_file`
        // — this function only resolves and validates the path); it must
        // resolve exactly like a one-level-missing target does, not fail
        // here before that creation ever gets a chance to run. `path` is
        // always a descendant of `root` (built by `join_relative`), so this
        // loop is guaranteed to terminate at `root` at the latest, which
        // was just confirmed to exist above.
        let mut existing = path.to_path_buf();
        let mut tail: Vec<std::ffi::OsString> = Vec::new();
        while !existing.exists() {
            let name = existing.file_name().ok_or_else(|| {
                ProjectError::Message(format!("invalid path: {}", path.display()))
            })?;
            tail.push(name.to_os_string());
            existing = existing
                .parent()
                .ok_or_else(|| {
                    ProjectError::Message(format!("invalid path: {}", path.display()))
                })?
                .to_path_buf();
        }
        let mut canonical = existing.canonicalize().map_err(ProjectError::Canonicalize)?;
        for part in tail.into_iter().rev() {
            canonical.push(part);
        }
        canonical
    };

    if !canonical.starts_with(&root) {
        return Err(ProjectError::PathEscape(canonical.display().to_string()));
    }
    Ok(canonical)
}

/// Relativize `absolute` against `root`. Returns `"."` when equal.
pub fn relative_to(root: &Path, absolute: &Path) -> Result<String, ProjectError> {
    let root = root
        .canonicalize()
        .map_err(ProjectError::Canonicalize)?;
    let absolute = absolute
        .canonicalize()
        .map_err(ProjectError::Canonicalize)?;

    if absolute == root {
        return Ok(".".to_string());
    }

    let rel = absolute
        .strip_prefix(&root)
        .map_err(|_| ProjectError::DocsOutsideRepo(absolute.display().to_string()))?;

    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => {
                return Err(ProjectError::DocsOutsideRepo(
                    absolute.display().to_string(),
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

/// Like `relative_to`, but tolerates `absolute` not existing (canonicalizes
/// its parent + rejoins the file name instead) — mirrors
/// `domain::workspace_index::relative_key_lenient`. Used by the incremental
/// index watcher, which must resolve a `FileId` for `Remove` events (and
/// for `Upserted` events that raced a deletion) where the path is already
/// gone.
pub fn relative_to_lenient(root: &Path, absolute: &Path) -> Result<String, ProjectError> {
    if absolute.exists() {
        return relative_to(root, absolute);
    }
    let root = root.canonicalize().map_err(ProjectError::Canonicalize)?;
    let parent = absolute
        .parent()
        .ok_or_else(|| ProjectError::Message(format!("invalid path: {}", absolute.display())))?;
    let name = absolute
        .file_name()
        .ok_or_else(|| ProjectError::Message(format!("invalid path: {}", absolute.display())))?;
    let parent = parent.canonicalize().map_err(ProjectError::Canonicalize)?;
    let absolute = parent.join(name);

    if absolute == root {
        return Ok(".".to_string());
    }
    let rel = absolute
        .strip_prefix(&root)
        .map_err(|_| ProjectError::DocsOutsideRepo(absolute.display().to_string()))?;

    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => {
                return Err(ProjectError::DocsOutsideRepo(
                    absolute.display().to_string(),
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

/// Plain string check, no filesystem access: `candidate` equals `prefix` or
/// starts with `"{prefix}/"`. `candidate` and `prefix` are already-relative,
/// `/`-separated strings (a `FileId` and a project's `docs_root`,
/// respectively) — used to filter search results to a subtree without
/// resolving each candidate against disk. An empty `prefix` is a
/// fail-closed sentinel: it matches nothing, not everything — a caller that
/// means "no filtering" represents that as `None` one level up, never as
/// `""` here.
pub fn is_under_relative_prefix(candidate: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return false;
    }
    candidate == prefix || candidate.starts_with(&format!("{prefix}/"))
}

/// Join `root` with a relative path that uses `/` separators. Rejects `..`.
pub fn join_relative(root: &Path, relative: &str) -> Result<PathBuf, ProjectError> {
    if relative.is_empty() || relative == "." {
        return Ok(root.to_path_buf());
    }

    let mut out = root.to_path_buf();
    for part in relative.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(ProjectError::PathEscape(relative.to_string()));
        }
        // A component Windows parses as a drive or UNC prefix (`C:`) makes
        // `PathBuf::push` *replace* the whole path instead of extending it, so
        // an absolute path from a caller silently left `root`:
        // `join_relative(r"C:\repo\docs", r"C:\Windows\win.ini")` used to
        // produce `C:Windows\win.ini`. On Linux the same input stays under the
        // root, so this also makes the contract platform-independent. Only
        // plain names may be appended.
        if Path::new(part).components().next() != Some(Component::Normal(OsStr::new(part))) {
            return Err(ProjectError::PathEscape(relative.to_string()));
        }
        out.push(part);
    }
    Ok(out)
}

/// `canonicalize`, already in the form a path may leave this process in — no
/// `\\?\` prefix. Every path that reaches the UI or a config file should come
/// from here rather than from a bare `canonicalize()`; comparisons are not
/// affected, because each "is this under that root" check canonicalizes both
/// sides itself.
pub fn canonicalize_plain(path: &Path) -> std::io::Result<PathBuf> {
    path.canonicalize().map(strip_verbatim)
}

/// Strip the Windows extended-length (`\\?\`) prefix that `canonicalize`
/// returns, turning `\\?\C:\repos\x` back into `C:\repos\x`.
///
/// Verbatim paths are not universally understood: libgit2 does not accept
/// them, and the string ends up persisted in `project.json` and the recent
/// projects list, where it leaks into every path the UI shows. Always run a
/// `canonicalize()` result through this before handing it onward. On non-
/// Windows targets there is no such prefix and the path passes through.
pub fn strip_verbatim(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::Prefix;

        let mut components = path.components();
        let Some(Component::Prefix(prefix)) = components.next() else {
            return path;
        };
        let rebuilt_root = match prefix.kind() {
            Prefix::VerbatimDisk(letter) => format!("{}:\\", letter as char),
            Prefix::VerbatimUNC(server, share) => {
                format!("\\\\{}\\{}\\", server.to_string_lossy(), share.to_string_lossy())
            }
            // `\\?\` with anything else (device paths) has no plain-path
            // equivalent — leave it alone rather than corrupt it.
            _ => return path,
        };
        let mut out = PathBuf::from(rebuilt_root);
        for component in components {
            // The root component is already in `rebuilt_root`.
            if !matches!(component, Component::RootDir) {
                out.push(component);
            }
        }
        out
    }
    #[cfg(not(windows))]
    {
        path
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn strip_verbatim_leaves_plain_paths_alone() {
        let plain = PathBuf::from(if cfg!(windows) { r"C:\repos\x" } else { "/repos/x" });
        assert_eq!(super::strip_verbatim(plain.clone()), plain);
    }

    #[test]
    fn join_relative_appends_plain_components_from_either_separator() {
        let root = PathBuf::from(if cfg!(windows) { r"C:\repo\docs" } else { "/repo/docs" });
        let expected = root.join("api").join("index.adoc");
        assert_eq!(join_relative(&root, "api/index.adoc").unwrap(), expected);
        assert_eq!(join_relative(&root, r"api\index.adoc").unwrap(), expected);
    }

    #[test]
    fn join_relative_rejects_a_drive_qualified_component() {
        // On Windows `PathBuf::push("C:")` replaces the path being built, so
        // without the guard an absolute argument escaped the root entirely —
        // `C:\Windows\win.ini` came out as the drive-relative
        // `C:Windows\win.ini`. Off Windows `C:` is an ordinary name and the
        // result simply stays under the root, which the guard must not break.
        let root = PathBuf::from(if cfg!(windows) { r"C:\repo\docs" } else { "/repo/docs" });
        let joined = join_relative(&root, r"C:\Windows\win.ini");
        if cfg!(windows) {
            assert!(matches!(joined, Err(ProjectError::PathEscape(_))));
            assert!(matches!(join_relative(&root, "C:"), Err(ProjectError::PathEscape(_))));
        } else {
            assert!(joined.unwrap().starts_with(&root));
        }
    }

    #[test]
    fn join_relative_treats_a_unc_looking_argument_as_relative() {
        // Leading separators are skipped rather than rejected, so this stays
        // inside the root instead of reaching a network share.
        let root = PathBuf::from(if cfg!(windows) { r"C:\repo\docs" } else { "/repo/docs" });
        let joined = join_relative(&root, r"\\server\share\file.txt").unwrap();
        assert_eq!(joined, root.join("server").join("share").join("file.txt"));
    }

    #[test]
    fn canonicalize_plain_hands_back_a_path_without_the_verbatim_prefix() {
        let dir = temp_dir();
        let plain = super::canonicalize_plain(&dir).unwrap();
        assert!(
            !plain.to_string_lossy().starts_with(r"\\?\"),
            "canonicalize_plain leaked a verbatim prefix: {}",
            plain.display()
        );
        // Still the same directory — stripping the prefix must not change
        // which path this is.
        assert_eq!(plain.canonicalize().unwrap(), dir.canonicalize().unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    #[cfg(windows)]
    #[test]
    fn strip_verbatim_removes_extended_length_prefix() {
        assert_eq!(
            super::strip_verbatim(PathBuf::from(r"\\?\C:\repos\clonned-repo")),
            PathBuf::from(r"C:\repos\clonned-repo")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn strip_verbatim_is_a_noop_off_windows() {
        // There is no verbatim prefix to strip; the string is just a path.
        let p = PathBuf::from(r"\\?\C:\repos");
        assert_eq!(super::strip_verbatim(p.clone()), p);
    }

    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    // A nanosecond timestamp alone can collide between parallel test threads
    // on a coarser system clock — see the same fix already applied in
    // `services::docs_fs`/`services::ai_tools`'s own `temp_dir` helpers. A
    // per-process counter guarantees uniqueness regardless of clock
    // resolution.
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("alfa-atlas-paths-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn relative_and_join_round_trip() {
        let root = temp_dir();
        let nested = root.join("src").join("docs");
        fs::create_dir_all(&nested).unwrap();

        let rel = relative_to(&root, &nested).unwrap();
        assert_eq!(rel, "src/docs");
        let joined = join_relative(&root, &rel).unwrap();
        assert_eq!(joined.canonicalize().unwrap(), nested.canonicalize().unwrap());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_parent_escape() {
        let root = temp_dir();
        assert!(join_relative(&root, "../outside").is_err());
        fs::remove_dir_all(&root).ok();
    }

    /// Regression test: a target several directories deep in a tree that
    /// doesn't exist yet at all (not just its immediate parent) must still
    /// resolve to a clean, contained canonical path — not fail with a raw
    /// `io::Error` from trying to canonicalize a nonexistent parent. This is
    /// what `writeFile`'s "missing parent directories are created
    /// automatically" behavior (and a plain `readFile`/`listFiles` on a
    /// wrong multi-segment path returning a clean `NotFound` instead of an
    /// opaque OS error) both depend on.
    #[test]
    fn ensure_under_resolves_a_target_several_missing_directories_deep() {
        let root = temp_dir();

        let target = root.join("brand").join("new").join("dir").join("file.adoc");
        let resolved = ensure_under(&root, &target).unwrap();
        assert_eq!(resolved, root.canonicalize().unwrap().join("brand/new/dir/file.adoc"));
        assert!(!resolved.exists());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ensure_under_still_rejects_a_deep_missing_target_that_escapes_root() {
        let root = temp_dir();
        let outside = root.parent().unwrap().join("escaped-brand-new-dir-outside-root").join("file.adoc");

        let err = ensure_under(&root, &outside).unwrap_err();
        assert!(matches!(err, ProjectError::PathEscape(_)));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn is_under_relative_prefix_matches_exact_and_nested() {
        assert!(is_under_relative_prefix("docs", "docs"));
        assert!(is_under_relative_prefix("docs/guide.adoc", "docs"));
    }

    #[test]
    fn is_under_relative_prefix_rejects_a_sibling() {
        assert!(!is_under_relative_prefix("docsx/guide.adoc", "docs"));
        assert!(!is_under_relative_prefix("src/main.rs", "docs"));
    }

    #[test]
    fn is_under_relative_prefix_empty_prefix_matches_nothing() {
        assert!(!is_under_relative_prefix("docs/guide.adoc", ""));
        assert!(!is_under_relative_prefix("", ""));
    }

    #[test]
    fn relative_to_lenient_delegates_when_the_path_still_exists() {
        let root = temp_dir();
        let file = root.join("a.txt");
        fs::write(&file, "x").unwrap();

        assert_eq!(relative_to_lenient(&root, &file).unwrap(), "a.txt");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn relative_to_lenient_resolves_a_since_deleted_file() {
        let root = temp_dir();
        let file = root.join("gone.txt");
        fs::write(&file, "x").unwrap();
        fs::remove_file(&file).unwrap();

        assert_eq!(relative_to_lenient(&root, &file).unwrap(), "gone.txt");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn relative_to_lenient_resolves_a_since_deleted_nested_file() {
        let root = temp_dir();
        let nested_dir = root.join("src").join("docs");
        fs::create_dir_all(&nested_dir).unwrap();
        let file = nested_dir.join("gone.md");
        fs::write(&file, "x").unwrap();
        fs::remove_file(&file).unwrap();

        assert_eq!(relative_to_lenient(&root, &file).unwrap(), "src/docs/gone.md");

        fs::remove_dir_all(&root).ok();
    }
}
