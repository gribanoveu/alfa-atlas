//! `listFiles` — the directory listing, rendered as an indented ASCII tree
//! rather than a flat list so the model can see nesting at a glance.

use std::path::PathBuf;

use crate::domain::ai_access::AiAccessMode;
use crate::domain::ai_tools::{ListFilesArgs, ToolError, ToolFileEntry, ToolScope};
use crate::domain::llm::LlmToolDefinition;
use crate::domain::paths;
use crate::domain::project_config::TreeNode;
use crate::infra::workspace_scanner;
use crate::services::docs_fs;

use super::super::resolve::{basename, resolve_subdir};

pub(super) fn list_files(scope: &ToolScope, args: ListFilesArgs) -> Result<Vec<ToolFileEntry>, ToolError> {
    let subdir = resolve_subdir(scope, args.path.as_deref())?;

    let mut entries = match scope.mode {
        AiAccessMode::DocsOnly => list_docs_only(scope, subdir.as_ref(), args.depth)?,
        AiAccessMode::FullRepo => {
            list_full_repo(scope, subdir.map(|(_, abs)| abs), args.depth)?
        }
    };

    if let Some(pattern) = args.pattern.as_deref() {
        let matcher = compile_glob(pattern)?;
        // Directories are always kept — `pattern` scopes which *files*
        // come back, not the navigable structure. This applies in `FullRepo`
        // mode too: `list_full_repo` reports real directory entries (see
        // `workspace_scanner::scan_all_entries_with_depth`), so a pattern
        // like "*.java" no longer hides the directories those files live in.
        entries.retain(|e| e.is_dir || matcher.is_match(basename(&e.path)));
    }

    Ok(entries)
}

pub(super) fn compile_glob(pattern: &str) -> Result<globset::GlobMatcher, ToolError> {
    globset::Glob::new(pattern)
        .map(|g| g.compile_matcher())
        .map_err(|e| ToolError::InvalidPattern(e.to_string()))
}

/// One directory level of the tree `render_file_tree` builds out of a flat
/// `ToolFileEntry` list — children sorted by name (`BTreeMap`) so the
/// rendered tree is deterministic regardless of the scan order the entries
/// arrived in. `is_file` is only meaningful on a leaf with no children of
/// its own; an intermediate path segment (inferred from some deeper entry's
/// path, never listed directly) is always rendered as a directory.
#[derive(Default)]
pub(super) struct TreeBuildNode {
    children: std::collections::BTreeMap<String, TreeBuildNode>,
    is_file: bool,
}

/// Renders a flat `listFiles` result as an indented ASCII tree (à la `tree(1)`)
/// instead of a JSON array — so the model can see the whole directory
/// structure and where each file sits at a glance, rather than reconstructing
/// it from N separate `path` strings. The first line is always `./` (the
/// access-mode root), never the on-disk folder name, so a docs-root folder
/// such as `asciidoc` is not mistaken for a child to prepend onto paths.
pub fn render_file_tree(entries: &[ToolFileEntry]) -> String {
    let mut root = TreeBuildNode::default();
    for entry in entries {
        let mut node = &mut root;
        let parts: Vec<&str> = entry.path.split('/').filter(|p| !p.is_empty()).collect();
        let Some((last, dirs)) = parts.split_last() else {
            continue;
        };
        for part in dirs {
            node = node.children.entry((*part).to_string()).or_default();
        }
        let leaf = node.children.entry((*last).to_string()).or_default();
        leaf.is_file = !entry.is_dir;
    }

    let mut out = String::from("./\n");
    render_tree_children(&root, "", &mut out);
    out
}

pub(super) fn render_tree_children(node: &TreeBuildNode, prefix: &str, out: &mut String) {
    let count = node.children.len();
    for (i, (name, child)) in node.children.iter().enumerate() {
        let is_last = i + 1 == count;
        let is_dir = !child.children.is_empty() || !child.is_file;
        out.push_str(prefix);
        out.push_str(if is_last { "└── " } else { "├── " });
        out.push_str(name);
        if is_dir {
            out.push('/');
        }
        out.push('\n');
        if is_dir {
            let child_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
            render_tree_children(child, &child_prefix, out);
        }
    }
}

pub(super) fn list_docs_only(
    scope: &ToolScope,
    subdir: Option<&(String, PathBuf)>,
    max_depth: Option<u32>,
) -> Result<Vec<ToolFileEntry>, ToolError> {
    let dir = subdir.map(|(_, abs)| abs.as_path()).unwrap_or(scope.root.as_path());
    let tree = docs_fs::list_docs_tree_scoped(&scope.root, dir, max_depth)?;
    let mut entries = Vec::new();
    flatten_tree(tree, &mut entries);
    Ok(entries)
}

pub(super) fn flatten_tree(nodes: Vec<TreeNode>, out: &mut Vec<ToolFileEntry>) {
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

pub(super) fn list_full_repo(
    scope: &ToolScope,
    scan_root: Option<PathBuf>,
    max_depth: Option<u32>,
) -> Result<Vec<ToolFileEntry>, ToolError> {
    let scan_root = scan_root.unwrap_or_else(|| scope.root.clone());
    let entries =
        workspace_scanner::scan_all_entries_with_depth(&scan_root, max_depth.map(|d| d as usize))?;
    entries
        .into_iter()
        .map(|e| {
            let rel = paths::relative_to(&scope.root, &e.path)?;
            Ok(ToolFileEntry {
                path: rel,
                is_dir: e.is_dir,
            })
        })
        .collect()
}

/// The `listFiles` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "listFiles".to_string(),
        description: "List files and directories under a path. `path` is relative to the current access-mode root: the documentation root in Docs-only mode, the repository root in Full-repo mode. Omit `path` or pass null to list that root. Use when directory structure is unknown — scaffold checks, \"what files exist here\", filename patterns. Do NOT use after `semanticSearch` already returned concrete file paths — read those with `readFile` instead. Do NOT use to explore code logic when search can locate the entry point directly. Returns an indented ASCII tree (directories end with `/`), not a flat list. The tree's first line is a display-only label for the current root (in Full-repo mode it may be the repository folder name); it is not part of any path argument. Child entries are relative to the current access-mode root. Do not manually prepend a documentation-root or repository-root segment to `path` — it is already relative to the current root. In Docs-only mode the listing includes only text documentation types (AsciiDoc, Markdown, JSON/YAML, PlantUML, Mermaid, plain text) — image binaries (.png/.svg/…) under the docs tree are intentionally omitted even when they exist on disk and are valid `image::` targets; do not treat their absence from this listing as a missing or dangling link (use check kind \"problems\" for missingImage). In Full-repo mode image files may appear; they are assets, not text to readFile."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": ["string", "null"],
                    "description": "Subdirectory relative to the current access-mode root (see tool description), or omitted/null for that root."
                },
                "depth": {
                    "type": ["integer", "null"],
                    "minimum": 0,
                    "description": "Maximum recursion depth below `path` (1 = only direct children, 0 = no descendant entries at all). Omit or null for no limit."
                },
                "pattern": {
                    "type": ["string", "null"],
                    "description": "Glob pattern (e.g. \"*.java\") matched against each entry's filename only, not its full path. Directories are always included regardless of this filter. Omit or null for no filtering."
                }
            },
            "required": []
        }),
        }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::domain::ai_access::AiAccessMode;
    use crate::domain::ai_tools::{ToolError, ToolFileEntry, ToolScope};
    use crate::services::ai_tools::testing::*;

    use super::*;

    #[test]
    fn render_file_tree_nests_by_path_and_sorts_children() {
        let entries = vec![
            ToolFileEntry { path: "build.gradle".to_string(), is_dir: false },
            ToolFileEntry { path: "src/main/java/com/example/Application.java".to_string(), is_dir: false },
            ToolFileEntry { path: "src/main/java/com/example/UserService.java".to_string(), is_dir: false },
            ToolFileEntry { path: "src/main/resources/application.yml".to_string(), is_dir: false },
            ToolFileEntry { path: "src/test/java/com/example/UserServiceTest.java".to_string(), is_dir: false },
        ];

        let tree = render_file_tree(&entries);

        assert_eq!(
            tree,
            "./\n\
             ├── build.gradle\n\
             └── src/\n    \
             ├── main/\n    │   \
             ├── java/\n    │   │   \
             └── com/\n    │   │       \
             └── example/\n    │   │           \
             ├── Application.java\n    │   │           \
             └── UserService.java\n    │   \
             └── resources/\n    │       \
             └── application.yml\n    \
             └── test/\n        \
             └── java/\n            \
             └── com/\n                \
             └── example/\n                    \
             └── UserServiceTest.java\n"
        );
    }

    #[test]
    fn render_file_tree_marks_explicit_empty_directory() {
        let entries = vec![ToolFileEntry { path: "empty".to_string(), is_dir: true }];
        assert_eq!(render_file_tree(&entries), "./\n└── empty/\n");
    }

    #[test]
    fn list_files_missing_several_directories_deep_returns_clean_not_found() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = list(&scope, Some("components/schemas")).unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)), "expected NotFound, got {err:?}");

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_docs_only_excludes_source_files() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let entries = list(&scope, None).unwrap();
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

        let entries = list(&scope, None).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"docs/intro.adoc"));
        assert!(paths.contains(&"docs/script.py"));
        assert!(paths.contains(&"src/main.rs"));

        fs::remove_dir_all(&repo).ok();
    }

    /// Regression test: `list_full_repo` used to hardcode `is_dir: false`
    /// on every entry, so a real directory was indistinguishable from a
    /// file in the model's eyes — see `workspace_scanner::
    /// scan_all_entries_with_depth`.
    #[test]
    fn list_files_full_repo_reports_directories() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let entries = list(&scope, None).unwrap();
        let is_dir_of = |p: &str| entries.iter().find(|e| e.path == p).map(|e| e.is_dir);
        assert_eq!(is_dir_of("docs"), Some(true));
        assert_eq!(is_dir_of("docs/intro.adoc"), Some(false));

        fs::remove_dir_all(&repo).ok();
    }

    /// An empty directory has zero files under it — under the old
    /// files-only scan it never appeared in the listing at all (not just
    /// mislabeled, genuinely invisible). Confirms
    /// `scan_all_entries_with_depth` surfaces it.
    #[test]
    fn list_files_full_repo_includes_empty_directory() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(repo.join("empty")).unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let entries = list(&scope, None).unwrap();
        let empty = entries.iter().find(|e| e.path == "empty");
        assert_eq!(empty.map(|e| e.is_dir), Some(true));

        fs::remove_dir_all(&repo).ok();
    }

    /// The key regression test for the `list_docs_only` walk-scoping fix:
    /// without it, `depth` would be measured from `docs_root` instead of
    /// from the requested `path`, silently producing wrong results.
    #[test]
    fn list_files_depth_is_relative_to_requested_subdir_not_root() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("a/b")).unwrap();
        fs::write(docs.join("a/direct.adoc"), "= Direct\n").unwrap();
        fs::write(docs.join("a/b/nested.adoc"), "= Nested\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let entries = list_scoped(&scope, Some("a"), Some(1), None).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"a/direct.adoc"));
        assert!(paths.contains(&"a/b"));
        // depth=1 relative to "a" excludes "a"'s grandchildren.
        assert!(!paths.contains(&"a/b/nested.adoc"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_depth_limits_recursion_in_docs_only() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("a/b")).unwrap();
        fs::write(docs.join("a/one.adoc"), "= One\n").unwrap();
        fs::write(docs.join("a/b/two.adoc"), "= Two\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let shallow = list_scoped(&scope, None, Some(2), None).unwrap();
        let shallow_paths: Vec<&str> = shallow.iter().map(|e| e.path.as_str()).collect();
        assert!(shallow_paths.contains(&"a/one.adoc"));
        assert!(!shallow_paths.contains(&"a/b/two.adoc"));

        let unlimited = list_scoped(&scope, None, None, None).unwrap();
        let unlimited_paths: Vec<&str> = unlimited.iter().map(|e| e.path.as_str()).collect();
        assert!(unlimited_paths.contains(&"a/b/two.adoc"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_depth_limits_recursion_in_full_repo() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(repo.join("src/nested")).unwrap();
        fs::write(repo.join("src/nested/deep.rs"), "fn deep() {}\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let shallow = list_scoped(&scope, None, Some(2), None).unwrap();
        let shallow_paths: Vec<&str> = shallow.iter().map(|e| e.path.as_str()).collect();
        assert!(shallow_paths.contains(&"src/main.rs"));
        assert!(!shallow_paths.iter().any(|p| p.ends_with("deep.rs")));

        let unlimited = list_scoped(&scope, None, None, None).unwrap();
        let unlimited_paths: Vec<&str> = unlimited.iter().map(|e| e.path.as_str()).collect();
        assert!(unlimited_paths.iter().any(|p| p.ends_with("deep.rs")));

        fs::remove_dir_all(&repo).ok();
    }

    /// Regression coverage for a real user report: `listFiles` on an
    /// existing, non-root subdirectory (`path` non-`None`, combined with
    /// `depth`) in Full-repo mode. `resolve_subdir`/`join_relative`/
    /// `ensure_under`/`relative_to` treat a one-segment and a multi-segment
    /// path identically (no depth-dependent logic anywhere in that chain),
    /// so this is expected to behave exactly like the root-path case above
    /// — this test exists to actually pin that down rather than leave it
    /// unverified.
    #[test]
    fn list_files_nested_path_with_depth_in_full_repo_returns_real_contents() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(repo.join("src/nested/deeper")).unwrap();
        fs::write(repo.join("src/nested/one.rs"), "fn one() {}\n").unwrap();
        fs::write(repo.join("src/nested/deeper/two.rs"), "fn two() {}\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let entries = list_scoped(&scope, Some("src/nested"), Some(1), None).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"src/nested/one.rs"));
        assert!(paths.contains(&"src/nested/deeper"));
        assert!(!paths.iter().any(|p| p.ends_with("two.rs")));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_depth_zero_returns_no_descendant_entries() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let entries = list_scoped(&scope, None, Some(0), None).unwrap();
        assert!(entries.is_empty());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_pattern_filters_by_basename_across_depths() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(repo.join("src/sub")).unwrap();
        fs::write(repo.join("src/a.java"), "class A {}\n").unwrap();
        fs::write(repo.join("src/sub/b.java"), "class B {}\n").unwrap();
        fs::write(repo.join("src/sub/c.txt"), "not java\n").unwrap();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::FullRepo);

        let entries = list_scoped(&scope, Some("src"), None, Some("*.java")).unwrap();
        let mut paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        paths.sort();
        // "src/sub" itself doesn't match "*.java" but is kept regardless —
        // `pattern` scopes which *files* come back, not the directory
        // structure (see `list_files_pattern_keeps_directory_entries` for
        // the same rule in Docs-only mode). This only became observable in
        // Full-repo mode once `list_full_repo` started reporting real
        // directory entries at all.
        assert_eq!(paths, vec!["src/a.java", "src/sub", "src/sub/b.java"]);

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_pattern_keeps_directory_entries() {
        let (repo, docs) = fixture_repo();
        fs::create_dir_all(docs.join("assets")).unwrap();
        fs::write(docs.join("assets/logo.png"), "not adoc").ok();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        // `assets` itself doesn't match "*.adoc", but must still be listed
        // — `pattern` scopes which files come back, not the directory
        // structure.
        let entries = list_scoped(&scope, None, None, Some("*.adoc")).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"assets"));
        assert!(paths.contains(&"intro.adoc"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn list_files_invalid_glob_pattern_returns_invalid_pattern_error() {
        let (repo, docs) = fixture_repo();
        let scope = ToolScope::for_project(&repo, &docs, AiAccessMode::DocsOnly);

        let err = list_scoped(&scope, None, None, Some("[")).unwrap_err();
        assert!(matches!(err, ToolError::InvalidPattern(_)));

        fs::remove_dir_all(&repo).ok();
    }
}
