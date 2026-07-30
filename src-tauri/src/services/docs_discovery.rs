use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::paths;
use crate::domain::project_config::{DocsCandidate, ProjectError};
use crate::domain::supported_files::{is_asciidoc, is_supported_file};

const MAX_DEPTH: usize = 6;
const MIN_SUPPORTED_FOR_DENSITY: usize = 2;

const SKIP_DIRS: &[&str] = &[
    ".git",
    ".atlas",
    "node_modules",
    "target",
    "dist",
    "build",
    ".idea",
    ".vscode",
    "vendor",
    "__pycache__",
    ".next",
    "coverage",
];

const NAMED_PATHS: &[&str] = &[
    "src/docs/asciidoc",
    "docs/asciidoc",
    "src/docs",
    "asciidoc",
    "docs",
    "doc",
    "documentation",
];

#[derive(Debug, Default, Clone)]
struct DirStats {
    supported: u32,
    asciidoc: u32,
    named_bonus: u32,
    depth: usize,
}

/// Find documentation root candidates under `scan_root` (typically the selected folder or repo).
pub fn find_candidates(
    repo_root: &Path,
    scan_root: &Path,
) -> Result<Vec<DocsCandidate>, ProjectError> {
    let repo_root = repo_root
        .canonicalize()
        .map_err(ProjectError::Canonicalize)?;
    let scan_root = scan_root
        .canonicalize()
        .map_err(ProjectError::Canonicalize)?;

    let mut stats: HashMap<PathBuf, DirStats> = HashMap::new();

    // Named path bonuses relative to repo root.
    for named in NAMED_PATHS {
        let candidate = repo_root.join(named);
        if candidate.is_dir() {
            let entry = stats.entry(candidate.clone()).or_default();
            entry.named_bonus = named_bonus(named);
        }
    }

    walk(
        &scan_root,
        &scan_root,
        0,
        &mut stats,
    )?;

    // If scan root itself looks like a docs leaf, ensure it is scored.
    if !stats.contains_key(&scan_root) {
        let (supported, asciidoc) = count_supported_immediate(&scan_root)?;
        if supported > 0 {
            stats.insert(
                scan_root.clone(),
                DirStats {
                    supported,
                    asciidoc,
                    named_bonus: 0,
                    depth: 0,
                },
            );
        }
    }

    let mut candidates: Vec<DocsCandidate> = stats
        .into_iter()
        .filter_map(|(path, st)| {
            if st.named_bonus == 0
                && st.supported < MIN_SUPPORTED_FOR_DENSITY as u32
                && st.asciidoc == 0
            {
                return None;
            }
            // Must stay under repo.
            if !path.starts_with(&repo_root) {
                return None;
            }
            let relative_path = paths::relative_to(&repo_root, &path).ok()?;
            let score = score(&st);
            let reason = reason(&st, &relative_path);
            Some(DocsCandidate {
                path: path.to_string_lossy().into_owned(),
                relative_path,
                score,
                reason,
            })
        })
        .collect();

    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });

    // Deduplicate by path, keep best.
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|c| seen.insert(c.path.clone()));

    Ok(candidates)
}

fn named_bonus(named: &str) -> u32 {
    match named {
        "src/docs/asciidoc" => 1000,
        "docs/asciidoc" => 900,
        "src/docs" => 700,
        "asciidoc" => 600,
        "docs" => 500,
        "doc" => 400,
        "documentation" => 350,
        _ => 100,
    }
}

fn score(st: &DirStats) -> u32 {
    st.named_bonus + st.asciidoc * 20 + st.supported * 5 + depth_bonus(st.depth)
}

fn depth_bonus(depth: usize) -> u32 {
    // Prefer shallower among equals; small nudge.
    10u32.saturating_sub(depth as u32)
}

fn reason(st: &DirStats, relative: &str) -> String {
    if st.named_bonus > 0 {
        format!("известное имя: {relative}")
    } else if st.asciidoc > 0 {
        format!("{} AsciiDoc-файлов", st.asciidoc)
    } else {
        format!("{} поддерживаемых файлов", st.supported)
    }
}

fn walk(
    scan_root: &Path,
    dir: &Path,
    depth: usize,
    stats: &mut HashMap<PathBuf, DirStats>,
) -> Result<(), ProjectError> {
    if depth > MAX_DEPTH {
        return Ok(());
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let mut supported = 0u32;
    let mut asciidoc = 0u32;

    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };

        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if SKIP_DIRS.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            walk(scan_root, &path, depth + 1, stats)?;
        } else if file_type.is_file() {
            let path_str = path.to_string_lossy();
            if is_supported_file(&path_str) {
                supported += 1;
                if is_asciidoc(&path_str) {
                    asciidoc += 1;
                }
            }
        }
    }

    if supported > 0 || asciidoc > 0 {
        let entry = stats.entry(dir.to_path_buf()).or_default();
        entry.supported = entry.supported.max(supported);
        entry.asciidoc = entry.asciidoc.max(asciidoc);
        entry.depth = depth;
    }

    Ok(())
}

fn count_supported_immediate(dir: &Path) -> Result<(u32, u32), ProjectError> {
    let mut supported = 0u32;
    let mut asciidoc = 0u32;
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok((0, 0)),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let s = path.to_string_lossy();
            if is_supported_file(&s) {
                supported += 1;
                if is_asciidoc(&s) {
                    asciidoc += 1;
                }
            }
        }
    }
    Ok((supported, asciidoc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("alfa-atlas-disc-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn prefers_named_asciidoc_path() {
        let root = temp_dir();
        let docs = root.join("src/docs/asciidoc");
        fs::create_dir_all(&docs).unwrap();
        fs::write(docs.join("index.adoc"), "= Doc\n").unwrap();
        fs::write(docs.join("a.json"), "{}\n").unwrap();

        // Noise elsewhere
        let src = root.join("src/main");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("App.java"), "class App {}\n").unwrap();

        let candidates = find_candidates(&root, &root).unwrap();
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].relative_path, "src/docs/asciidoc");

        fs::remove_dir_all(&root).ok();
    }
}
