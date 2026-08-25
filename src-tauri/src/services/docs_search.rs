//! Exact regex content search under a filesystem root — shared by the AI
//! `grep` tool and the user-facing `docs_search` IPC. Walk is
//! gitignore-aware; binary / oversized / non-UTF-8 files are skipped.

use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::ai_tools::{GrepArgs, GrepMatch};
use crate::domain::docs_search::{DocsSearchError, GrepResultsPayload};
use crate::domain::paths;
use crate::infra::workspace_scanner;

const DEFAULT_GREP_RESULTS: usize = 50;
const MAX_GREP_RESULTS: usize = 200;
const MAX_GREP_FILE_BYTES: u64 = 1_048_576;
const GREP_BINARY_SNIFF_BYTES: usize = 8_192;
const GREP_LINE_MAX_CHARS: usize = 300;

/// Search under an already-resolved root (docs root for the UI; `ToolScope.root`
/// for the AI tool). Paths in results are relative to `root`, `/`-separated.
pub fn search_under_root(
    root: &Path,
    args: &GrepArgs,
) -> Result<GrepResultsPayload, DocsSearchError> {
    let max_results = args
        .max_results
        .unwrap_or(DEFAULT_GREP_RESULTS)
        .clamp(1, MAX_GREP_RESULTS);

    let mut builder = regex::RegexBuilder::new(&args.pattern);
    builder.case_insensitive(args.case_insensitive.unwrap_or(false));
    let re = builder
        .build()
        .map_err(|e| DocsSearchError::InvalidPattern(e.to_string()))?;

    let glob = match args.glob.as_deref() {
        Some(p) if !p.is_empty() => Some(compile_glob(p)?),
        _ => None,
    };

    let target = resolve_search_target(root, args.path.as_deref())?;
    let mut matches = Vec::new();
    let mut truncated = false;

    match target {
        SearchTarget::File(path) => {
            let rel = paths::relative_to(root, &path)?.replace('\\', "/");
            truncated = grep_one_file(
                &path,
                &rel,
                &re,
                glob.as_ref(),
                max_results,
                &mut matches,
            );
        }
        SearchTarget::Dir(scan_root) => {
            let files = workspace_scanner::scan_all_with_depth(&scan_root, None)?;
            'files: for scanned in files {
                let rel = match paths::relative_to(root, &scanned.path) {
                    Ok(r) => r.replace('\\', "/"),
                    Err(_) => continue,
                };
                let hit = grep_one_file(
                    &scanned.path,
                    &rel,
                    &re,
                    glob.as_ref(),
                    max_results,
                    &mut matches,
                );
                if hit {
                    truncated = true;
                    break 'files;
                }
            }
        }
    }

    Ok(GrepResultsPayload {
        matches,
        truncated,
    })
}

/// User-facing entry: canonicalize `docs_root`, then search only under it.
pub fn search_docs(docs_root: &str, args: &GrepArgs) -> Result<GrepResultsPayload, DocsSearchError> {
    let path = Path::new(docs_root);
    if !path.is_dir() {
        return Err(DocsSearchError::NotFound(docs_root.to_string()));
    }
    let root = path
        .canonicalize()
        .map_err(DocsSearchError::Io)?;
    search_under_root(&root, args)
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn compile_glob(pattern: &str) -> Result<globset::GlobMatcher, DocsSearchError> {
    globset::Glob::new(pattern)
        .map(|g| g.compile_matcher())
        .map_err(|e| DocsSearchError::InvalidPattern(e.to_string()))
}

/// `path` is a file (search only that file) or a directory (walk it).
/// `None` / `.` / empty → the whole `root`.
enum SearchTarget {
    Dir(PathBuf),
    File(PathBuf),
}

fn resolve_search_target(
    root: &Path,
    path: Option<&str>,
) -> Result<SearchTarget, DocsSearchError> {
    let Some(path) = path.filter(|p| !p.is_empty() && *p != ".") else {
        return Ok(SearchTarget::Dir(root.to_path_buf()));
    };
    let joined = paths::join_relative(root, path)?;
    let canonical = paths::ensure_under(root, &joined)?;
    if canonical.is_file() {
        return Ok(SearchTarget::File(canonical));
    }
    if canonical.is_dir() {
        return Ok(SearchTarget::Dir(canonical));
    }
    Err(DocsSearchError::NotFound(path.to_string()))
}

/// Pushes line hits from one file. Returns `true` when `max_results` is hit
/// (caller should stop walking).
fn grep_one_file(
    abs: &Path,
    rel: &str,
    re: &regex::Regex,
    glob: Option<&globset::GlobMatcher>,
    max_results: usize,
    matches: &mut Vec<GrepMatch>,
) -> bool {
    if let Some(matcher) = glob {
        if !matcher.is_match(basename(rel)) {
            return false;
        }
    }
    let Ok(meta) = fs::metadata(abs) else {
        return false;
    };
    if meta.len() > MAX_GREP_FILE_BYTES {
        return false;
    }
    let Ok(bytes) = fs::read(abs) else {
        return false;
    };
    let sniff_len = GREP_BINARY_SNIFF_BYTES.min(bytes.len());
    if bytes[..sniff_len].contains(&0) {
        return false;
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return false;
    };
    for (idx, line) in content.lines().enumerate() {
        if !re.is_match(line) {
            continue;
        }
        if matches.len() >= max_results {
            return true;
        }
        matches.push(GrepMatch {
            path: rel.to_string(),
            line: (idx + 1) as u32,
            text: truncate_grep_line(line),
        });
    }
    false
}

fn truncate_grep_line(line: &str) -> String {
    if line.chars().count() <= GREP_LINE_MAX_CHARS {
        return line.to_string();
    }
    let truncated: String = line.chars().take(GREP_LINE_MAX_CHARS).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture_repo() -> (PathBuf, PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let repo = std::env::temp_dir().join(format!("alfa-atlas-docs-search-{nanos}-{n}"));
        let docs = repo.join("docs");
        fs::create_dir_all(docs.join("nested")).unwrap();
        fs::create_dir_all(repo.join("src")).unwrap();
        (repo, docs)
    }

    #[test]
    fn docs_search_finds_hits_under_docs_and_excludes_outside() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("guide.adoc"), "= Guide\ncall Needle.here()\nmore\n").unwrap();
        fs::write(repo.join("src/main.rs"), "fn Needle() {}\n").unwrap();

        let payload = search_docs(
            docs.to_str().unwrap(),
            &GrepArgs {
                pattern: "Needle".to_string(),
                path: None,
                glob: None,
                case_insensitive: None,
                max_results: None,
            },
        )
        .unwrap();

        assert!(!payload.truncated);
        assert_eq!(payload.matches.len(), 1);
        assert_eq!(payload.matches[0].path, "guide.adoc");
        assert_eq!(payload.matches[0].line, 2);
        assert!(payload.matches[0].text.contains("Needle"));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn docs_search_rejects_invalid_regex() {
        let (repo, docs) = fixture_repo();
        let err = search_docs(
            docs.to_str().unwrap(),
            &GrepArgs {
                pattern: "(unclosed".to_string(),
                path: None,
                glob: None,
                case_insensitive: None,
                max_results: None,
            },
        )
        .unwrap_err();
        assert!(
            matches!(err, DocsSearchError::InvalidPattern(_)),
            "got {err:?}"
        );
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn docs_search_truncates_when_max_results_is_hit() {
        let (repo, docs) = fixture_repo();
        let mut body = String::new();
        for i in 0..10 {
            body.push_str(&format!("hit {i}\n"));
        }
        fs::write(docs.join("many.adoc"), body).unwrap();

        let payload = search_docs(
            docs.to_str().unwrap(),
            &GrepArgs {
                pattern: "hit".to_string(),
                path: None,
                glob: None,
                case_insensitive: None,
                max_results: Some(3),
            },
        )
        .unwrap();

        assert!(payload.truncated);
        assert_eq!(payload.matches.len(), 3);
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn docs_search_respects_glob_filter() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("a.adoc"), "Needle in adoc\n").unwrap();
        fs::write(docs.join("b.json"), "Needle in json\n").unwrap();

        let payload = search_docs(
            docs.to_str().unwrap(),
            &GrepArgs {
                pattern: "Needle".to_string(),
                path: None,
                glob: Some("*.adoc".to_string()),
                case_insensitive: None,
                max_results: None,
            },
        )
        .unwrap();

        assert_eq!(payload.matches.len(), 1);
        assert_eq!(payload.matches[0].path, "a.adoc");
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn docs_search_path_may_be_a_file() {
        let (repo, docs) = fixture_repo();
        fs::write(docs.join("a.adoc"), "Needle in a\n").unwrap();
        fs::write(docs.join("b.adoc"), "Needle in b\n").unwrap();

        let payload = search_docs(
            docs.to_str().unwrap(),
            &GrepArgs {
                pattern: "Needle".to_string(),
                path: Some("a.adoc".to_string()),
                glob: None,
                case_insensitive: None,
                max_results: None,
            },
        )
        .unwrap();

        assert_eq!(payload.matches.len(), 1);
        assert_eq!(payload.matches[0].path, "a.adoc");
        assert!(payload.matches[0].text.contains("Needle in a"));
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn docs_search_missing_path_is_not_found() {
        let (repo, docs) = fixture_repo();
        let err = search_docs(
            docs.to_str().unwrap(),
            &GrepArgs {
                pattern: "Needle".to_string(),
                path: Some("nope.adoc".to_string()),
                glob: None,
                case_insensitive: None,
                max_results: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, DocsSearchError::NotFound(_)));
        fs::remove_dir_all(&repo).ok();
    }
}
