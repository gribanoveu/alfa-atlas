//! Executor for the read-only tools a future AI harness will call. This is
//! the enforcement point for `AiAccessMode`: every function here resolves
//! containment against `scope.root` via `domain::paths` — the same
//! primitives `services::docs_fs` uses — so a caller can never widen access
//! by passing an unexpected path, only by the `ToolScope` itself having been
//! constructed with the wider root.

use std::fs;
use std::path::PathBuf;

use crate::domain::ai_access::AiAccessMode;
use crate::domain::ai_tools::{
    is_tool_allowed, ListFilesArgs, ReadFileArgs, ToolError, ToolFileEntry, ToolName, ToolScope,
};
use crate::domain::paths;
use crate::domain::project_config::TreeNode;
use crate::infra::workspace_scanner;
use crate::services::docs_fs;

pub fn list_files(
    scope: &ToolScope,
    args: ListFilesArgs,
) -> Result<Vec<ToolFileEntry>, ToolError> {
    if !is_tool_allowed(scope.mode, ToolName::ListFiles) {
        return Err(ToolError::NotAllowed(ToolName::ListFiles));
    }

    let subdir = resolve_subdir(scope, args.path.as_deref())?;

    match scope.mode {
        AiAccessMode::DocsOnly => {
            list_docs_only(scope, subdir.as_ref().map(|(rel, _)| rel.as_str()))
        }
        AiAccessMode::FullRepo => list_full_repo(scope, subdir.map(|(_, abs)| abs)),
    }
}

pub fn read_file(scope: &ToolScope, args: ReadFileArgs) -> Result<String, ToolError> {
    if !is_tool_allowed(scope.mode, ToolName::ReadFile) {
        return Err(ToolError::NotAllowed(ToolName::ReadFile));
    }

    // No extension filtering here, unlike `docs_fs::read_project_file` —
    // the tool boundary is containment under `scope.root` alone. In
    // `FullRepo` mode the harness must be able to read source files, which
    // aren't in `is_supported_file`'s doc-format list.
    let joined = paths::join_relative(&scope.root, &args.path)?;
    let canonical = paths::ensure_under(&scope.root, &joined)?;
    if !canonical.exists() {
        return Err(ToolError::NotFound(args.path));
    }
    if !canonical.is_file() {
        return Err(ToolError::NotAFile(args.path));
    }
    fs::read_to_string(&canonical).map_err(ToolError::Io)
}

/// Validates an optional subdirectory argument once, shared by both mode
/// branches: returns its root-relative string form (for the docs-only
/// prefix filter) and its canonical absolute form (for the full-repo scan
/// root).
fn resolve_subdir(
    scope: &ToolScope,
    path: Option<&str>,
) -> Result<Option<(String, PathBuf)>, ToolError> {
    let Some(path) = path else {
        return Ok(None);
    };
    if path.is_empty() || path == "." {
        return Ok(None);
    }
    let joined = paths::join_relative(&scope.root, path)?;
    let canonical = paths::ensure_under(&scope.root, &joined)?;
    if !canonical.is_dir() {
        return Err(ToolError::NotFound(path.to_string()));
    }
    let rel = paths::relative_to(&scope.root, &canonical)?;
    Ok(Some((rel, canonical)))
}

fn list_docs_only(
    scope: &ToolScope,
    subdir_rel: Option<&str>,
) -> Result<Vec<ToolFileEntry>, ToolError> {
    let tree = docs_fs::list_docs_tree(&scope.root.to_string_lossy())?;
    let mut entries = Vec::new();
    flatten_tree(tree, &mut entries);

    let Some(prefix) = subdir_rel else {
        return Ok(entries);
    };
    let with_slash = format!("{prefix}/");
    entries.retain(|e| e.path == prefix || e.path.starts_with(&with_slash));
    Ok(entries)
}

fn flatten_tree(nodes: Vec<TreeNode>, out: &mut Vec<ToolFileEntry>) {
    for node in nodes {
        out.push(ToolFileEntry {
            path: node.path,
            is_dir: node.is_dir,
        });
        if let Some(children) = node.children {
            flatten_tree(children, out);
        }
    }
}

fn list_full_repo(
    scope: &ToolScope,
    scan_root: Option<PathBuf>,
) -> Result<Vec<ToolFileEntry>, ToolError> {
    let scan_root = scan_root.unwrap_or_else(|| scope.root.clone());
    let files = workspace_scanner::scan_all(&scan_root)?;
    files
        .into_iter()
        .map(|f| {
            let rel = paths::relative_to(&scope.root, &f.path)?;
            Ok(ToolFileEntry {
                path: rel,
                is_dir: false,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Builds a `repo_root/docs/...` + `repo_root/src/...` fixture and
    /// returns `(repo_root, docs_root)`, both canonicalized.
    fn fixture_repo() -> (PathBuf, PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let repo = std::env::temp_dir().join(format!("alfa-atlas-ai-tools-{nanos}"));
        let docs = repo.join("docs");
        let src = repo.join("src");
        fs::create_dir_all(&docs).unwrap();
        fs::create_dir_all(&src).unwrap();
        fs::write(docs.join("intro.adoc"), "= Intro\n").unwrap();
        fs::write(docs.join("script.py"), "print('unsupported ext')\n").unwrap();
        fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();

        let repo = repo.canonicalize().unwrap();
        let docs = docs.canonicalize().unwrap();
        (repo, docs)
    }

    #[test]
    fn read_file_inside_docs_root_succeeds_in_docs_only() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let content = read_file(
            &scope,
            ReadFileArgs {
                path: "intro.adoc".to_string(),
            },
        )
        .unwrap();
        assert_eq!(content, "= Intro\n");
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_on_a_directory_returns_not_a_file() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = read_file(
            &scope,
            ReadFileArgs {
                path: ".".to_string(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::NotAFile(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_rejects_parent_escape_in_both_modes() {
        let (repo, docs) = fixture_repo();

        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let err = read_file(
            &docs_only,
            ReadFileArgs {
                path: "../src/main.rs".to_string(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let err = read_file(
            &full_repo,
            ReadFileArgs {
                path: "../outside.txt".to_string(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn read_file_same_relative_path_resolves_against_different_roots_by_mode() {
        let (repo, docs) = fixture_repo();

        // "src/main.rs" only exists under `repo`, not under `docs` — so the
        // same relative path is simply absent from the docs-only root
        // (there is no `docs/src/main.rs`), while it resolves fine once the
        // scope root widens to the whole repo. This is `ToolScope`'s mode
        // switch doing its job, not a `..`-escape (that's covered above).
        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let err = read_file(
            &docs_only,
            ReadFileArgs {
                path: "src/main.rs".to_string(),
            },
        );
        assert!(err.is_err());

        let full_repo = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);
        let content = read_file(
            &full_repo,
            ReadFileArgs {
                path: "src/main.rs".to_string(),
            },
        )
        .unwrap();
        assert_eq!(content, "fn main() {}\n");

        fs::remove_dir_all(&repo).ok();
    }

    /// `join_relative`'s `..`-rejection only catches lexical traversal; the
    /// real defense against a path that resolves outside the root by other
    /// means (e.g. a symlink) is `ensure_under`'s canonicalize+`starts_with`
    /// check. Exercise that directly so the containment guarantee isn't
    /// only proven for the lexical case.
    #[cfg(unix)]
    #[test]
    fn read_file_rejects_symlink_escaping_docs_root() {
        let (repo, docs) = fixture_repo();
        std::os::unix::fs::symlink(repo.join("src/main.rs"), docs.join("leak.adoc")).unwrap();

        let docs_only = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);
        let err = read_file(
            &docs_only,
            ReadFileArgs {
                path: "leak.adoc".to_string(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, ToolError::PathEscape(_)));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_docs_only_excludes_source_files() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let entries = list_files(&scope, ListFilesArgs { path: None }).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"intro.adoc"));
        assert!(!paths.contains(&"script.py"));
        assert!(!paths.iter().any(|p| p.ends_with("main.rs")));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_full_repo_includes_source_files() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let entries = list_files(&scope, ListFilesArgs { path: None }).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"docs/intro.adoc"));
        assert!(paths.contains(&"docs/script.py"));
        assert!(paths.contains(&"src/main.rs"));

        fs::remove_dir_all(&repo).ok();
    }
}
