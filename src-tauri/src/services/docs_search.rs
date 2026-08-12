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

    let subdir = resolve_subdir(root, args.path.as_deref())?;
    let scan_root = subdir.unwrap_or_else(|| root.to_path_buf());

    let files = workspace_scanner::scan_all_with_depth(&scan_root, None)?;
    let mut matches = Vec::new();
    let mut truncated = false;

    'files: for scanned in files {
        if scanned.size > MAX_GREP_FILE_BYTES {
            continue;
        }
        let rel = match paths::relative_to(root, &scanned.path) {
            Ok(r) => r.replace('\\', "/"),
            Err(_) => continue,
        };
        if let Some(ref matcher) = glob {
            if !matcher.is_match(basename(&rel)) {
                continue;
            }
        }

        let Ok(bytes) = fs::read(&scanned.path) else {
            continue;
        };
        let sniff_len = GREP_BINARY_SNIFF_BYTES.min(bytes.len());
        if bytes[..sniff_len].contains(&0) {
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };

        for (idx, line) in content.lines().enumerate() {
            if !re.is_match(line) {
                continue;
            }
            if matches.len() >= max_results {
                truncated = true;
                break 'files;
            }
            matches.push(GrepMatch {
                path: rel.clone(),
                line: (idx + 1) as u32,
                text: truncate_grep_line(line),
            });
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

fn resolve_subdir(
    root: &Path,
    path: Option<&str>,
) -> Result<Option<PathBuf>, DocsSearchError> {
    let Some(path) = path else {
        return Ok(None);
    };
    if path.is_empty() || path == "." {
        return Ok(None);
    }
    let joined = paths::join_relative(root, path)?;
    let canonical = paths::ensure_under(root, &joined)?;
    if !canonical.is_dir() {
        return Err(DocsSearchError::NotFound(path.to_string()));
    }
    Ok(Some(canonical))
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
}
