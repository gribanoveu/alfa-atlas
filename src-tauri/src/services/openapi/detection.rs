use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::openapi::{OpenApiError, SpecsRepoInfo};
use crate::domain::paths;

use super::bundle::{ext_of, parse_generic};

/// Conventional structural subfolders of this OpenAPI multi-file spec
/// convention. None are individually required — real-world spec repos
/// sometimes omit one (e.g. no `parameters/` if no operation needs extra
/// parameters) — each is instead an independent scored signal, see
/// [`score_specs_signals`].
pub const KNOWN_SUBDIRS: [&str; 4] = ["schemas", "responses", "parameters", "operations"];

pub const ENTRY_FILE_POINTS: u32 = 40;
pub const SUBDIR_POINTS: u32 = 15;

/// Minimum [`score_specs_signals`] score for [`specs_root_signature`] to
/// treat a directory as a spec root worth bundling for the API Explorer:
/// the entry document plus at least one structural subfolder. The entry
/// document itself is effectively still mandatory — without one there is
/// nothing to bundle, and `score_specs_signals` never returns an `entry`
/// without having found it, regardless of how the subfolder points add up.
pub const DETECTION_THRESHOLD: u32 = ENTRY_FILE_POINTS + SUBDIR_POINTS;

const SPEC_EXTS: [&str; 3] = ["yaml", "yml", "json"];

/// The entry document found directly inside a detected specs root, plus
/// whatever `info.title`/`info.version` it declares.
pub struct SpecsRootSignature {
    pub entry_path: PathBuf,
    pub title: Option<String>,
    pub version: Option<String>,
}

/// How strongly a directory looks like an OpenAPI multi-file spec root,
/// as an additive score rather than a pass/fail gate on a fixed required
/// set. Used by the docs-root discovery heuristic to weigh "this is a spec
/// root" against its own "this is a docs root" score for the very same
/// directory, and let whichever is higher win.
pub struct SpecsSignal {
    pub score: u32,
    pub entry: Option<SpecsRootSignature>,
}

/// Scores `dir` on how strongly it looks like an OpenAPI multi-file spec
/// root: [`ENTRY_FILE_POINTS`] for a YAML/JSON file directly inside `dir`
/// (not recursing into subfolders) with a top-level `openapi:`/`swagger:`
/// key, plus [`SUBDIR_POINTS`] for each of [`KNOWN_SUBDIRS`] actually
/// present. No single signal is required for a nonzero score.
pub fn score_specs_signals(dir: &Path) -> SpecsSignal {
    if !dir.is_dir() {
        return SpecsSignal {
            score: 0,
            entry: None,
        };
    }

    let entry = find_entry_file(dir);
    let mut score = if entry.is_some() { ENTRY_FILE_POINTS } else { 0 };
    for sub in KNOWN_SUBDIRS {
        if dir.join(sub).is_dir() {
            score += SUBDIR_POINTS;
        }
    }
    SpecsSignal { score, entry }
}

/// Scans the files directly inside `dir` (not recursing into subfolders)
/// for one with a top-level `openapi:`/`swagger:` key.
fn find_entry_file(dir: &Path) -> Option<SpecsRootSignature> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.path())
        .filter(|p| {
            ext_of(p)
                .map(|ext| SPEC_EXTS.contains(&ext.as_str()))
                .unwrap_or(false)
        })
        .collect();
    entries.sort();

    for path in entries {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(ext) = ext_of(&path) else { continue };
        let Ok(value) = parse_generic(&text, &ext) else {
            continue;
        };
        let Some(obj) = value.as_object() else {
            continue;
        };
        if obj.contains_key("openapi") || obj.contains_key("swagger") {
            let title = value
                .pointer("/info/title")
                .and_then(|v| v.as_str())
                .map(String::from);
            let version = value
                .pointer("/info/version")
                .and_then(|v| v.as_str())
                .map(String::from);
            return Some(SpecsRootSignature {
                entry_path: path,
                title,
                version,
            });
        }
    }

    None
}

/// Checks whether `dir` scores highly enough ([`score_specs_signals`] >=
/// [`DETECTION_THRESHOLD`]) to be treated as a spec root worth bundling.
/// Shared conceptually with the docs-root discovery heuristic (both are
/// built on `score_specs_signals`), but this one applies a fixed threshold
/// rather than comparing against a competing docs score, since it's used to
/// gate a specific feature (the API Explorer) rather than to rank candidates.
pub fn specs_root_signature(dir: &Path) -> Option<SpecsRootSignature> {
    let signal = score_specs_signals(dir);
    if signal.score < DETECTION_THRESHOLD {
        return None;
    }
    signal.entry
}

/// Detects whether `repo_root/specs` follows this OpenAPI multi-file spec
/// convention closely enough ([`specs_root_signature`]) to bundle, and if
/// so, finds the entry document. This gate is independent of the docs-root
/// discovery heuristic (`docs_discovery.rs`), which separately weighs the
/// same underlying score against a competing "looks like documentation"
/// score to decide what to suggest as a project's docs root.
pub fn detect_specs_repo(repo_root: &Path) -> Result<Option<SpecsRepoInfo>, OpenApiError> {
    let specs_root = repo_root.join("specs");
    let Some(sig) = specs_root_signature(&specs_root) else {
        return Ok(None);
    };

    let entry_file = paths::relative_to(repo_root, &sig.entry_path)?;
    let specs_root_canonical = specs_root
        .canonicalize()
        .map_err(crate::domain::project_config::ProjectError::Canonicalize)?;
    Ok(Some(SpecsRepoInfo {
        specs_root: specs_root_canonical.to_string_lossy().into_owned(),
        entry_file,
        title: sig.title,
        version: sig.version,
    }))
}
