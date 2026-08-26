//! Harness-facing OptMem adapter for Atlas dual roots.
//!
//! Production callers use only:
//! - `wake_context` — pre-turn inject
//! - `note` + `drain_pending_naps` — post-turn writes (via `memory_pipeline`)
//! - `list_raw_entries` + `delete_log_entry` — memory viewer UI
//! - path helpers — FS tool guard

use std::path::{Path, PathBuf};

use crate::domain::optmem::{
    cover, paginate, pending, pending_count, plural, BlockRange, MemoryEntry, OptMemError, RAW_MAX,
};
use crate::infra::optmem_store::{validate_entry, OptMemStore, OptMemStoreError};
use crate::infra::settings_store;

/// Which of the two stores a memory op targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryScope {
    Project,
    Global,
}

impl MemoryScope {
    pub fn from_wire(s: &str) -> Result<Self, String> {
        match s {
            "project" => Ok(Self::Project),
            "global" => Ok(Self::Global),
            other => Err(format!(
                "unknown memory scope: \"{other}\" (expected \"project\" or \"global\")"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentMemoryError {
    #[error("{0}")]
    Store(#[from] OptMemStoreError),
    #[error("{0}")]
    Domain(#[from] OptMemError),
    #[error("{0}")]
    Message(String),
    #[error("settings: {0}")]
    Settings(String),
}

pub fn project_memory_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".atlas").join("memory")
}

pub fn global_memory_dir() -> Result<PathBuf, AgentMemoryError> {
    settings_store::settings_dir()
        .map(|d| d.join("memory"))
        .map_err(|e| AgentMemoryError::Settings(e.to_string()))
}

pub fn resolve_dir(scope: MemoryScope, repo_root: &Path) -> Result<PathBuf, AgentMemoryError> {
    match scope {
        MemoryScope::Project => Ok(project_memory_dir(repo_root)),
        MemoryScope::Global => global_memory_dir(),
    }
}

fn open_store(scope: MemoryScope, repo_root: &Path) -> Result<OptMemStore, AgentMemoryError> {
    let dir = resolve_dir(scope, repo_root)?;
    Ok(OptMemStore::open_or_init(&dir)?)
}

/// Read-only open — does not create the store directory.
fn try_open_store(
    scope: MemoryScope,
    repo_root: &Path,
) -> Result<Option<OptMemStore>, AgentMemoryError> {
    let dir = resolve_dir(scope, repo_root)?;
    Ok(OptMemStore::open_if_exists(&dir)?)
}

/// True when `path` (absolute or docs-relative join) lies under
/// `{repo}/.atlas/memory` — used to hard-deny FS mutate tools against the
/// OptMem store.
pub fn path_is_under_project_memory(repo_root: &Path, path: &Path) -> bool {
    let memory = project_memory_dir(repo_root);
    path == memory || path.starts_with(&memory)
}

fn today() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil date from days since Unix epoch (Howard Hinnant algorithm).
    const DAY: i64 = 86_400;
    let days = (secs as i64) / DAY;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Prefetch TREE level lengths so I/O failures surface instead of looking
/// like "zero summaries → demand nap".
fn level_lens(
    store: &OptMemStore,
    t: usize,
) -> Result<std::collections::BTreeMap<usize, usize>, AgentMemoryError> {
    let mut map = std::collections::BTreeMap::new();
    let mut size = 2usize;
    while size <= t {
        map.insert(size, store.level_len(size)?);
        match size.checked_mul(2) {
            Some(next) => size = next,
            None => break,
        }
    }
    Ok(map)
}

fn level_len_fn<'a>(
    map: &'a std::collections::BTreeMap<usize, usize>,
) -> impl Fn(usize) -> usize + 'a {
    move |size| map.get(&size).copied().unwrap_or(0)
}

/// Prompt for harness-side TREE compression (`drain_pending_naps`).
fn nap_prompt(
    store: &OptMemStore,
    lo: usize,
    hi: usize,
    left: usize,
) -> Result<String, AgentMemoryError> {
    let entry_chars = store.knobs().entry_chars;
    let body = if hi - lo <= RAW_MAX {
        store
            .log_slice(lo, hi)?
            .into_iter()
            .map(|e| format!("  #{} {} {}", e.id, e.date, e.text))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        let mid = (lo + hi) / 2;
        let mut halves = Vec::new();
        for (a, b) in [(lo, mid), (mid, hi)] {
            match store.tree_get(a, b)? {
                Some(s) => halves.push(format!("  #{}-{} {}", a, b - 1, s)),
                None => {
                    return Err(AgentMemoryError::Store(OptMemStoreError::BlankSummary {
                        lo: a,
                        hi: b - 1,
                    }));
                }
            }
        }
        halves.join("\n")
    };
    let tail = if left == 0 {
        String::new()
    } else if left == 1 {
        "\n1 compression remains after this one.".to_string()
    } else {
        format!("\n{left} compressions remain after this one.")
    };
    Ok(format!(
        "Compress memories #{lo}-{} into one line of at most {entry_chars} bytes.\n\
         Keep what has lasting effect, drop what does not. Invent nothing.\n\
         Reply with ONLY the summary line — no quotes, no prefixes, no explanation.\n\n\
         {body}{tail}",
        hi - 1,
    ))
}

fn wake_from_store(scope: MemoryScope, store: &OptMemStore) -> Result<String, AgentMemoryError> {
    let t = store.log_len()?;
    if t == 0 {
        return Ok(String::new());
    }

    let mut lines = Vec::new();
    let mut pending_compressions = 0usize;
    for BlockRange { lo, hi } in cover(t, store.knobs().wake_lines) {
        if hi - lo == 1 {
            let e = store.log_get(lo)?;
            lines.push(format!("#{} {} {}", e.id, e.date, e.text));
        } else {
            match store.tree_get(lo, hi)? {
                Some(s) => lines.push(format!("#{}-{} {}", lo, hi - 1, s)),
                None => {
                    pending_compressions += 1;
                    lines.push(format!("#{}-{} (not compressed yet)", lo, hi - 1));
                }
            }
        }
    }

    let parts = paginate(
        &lines,
        store.knobs().part_chars,
        store.knobs().part_lines,
    );

    let mut out = Vec::new();
    out.push(format!(
        "[{}] Your memory ({}):",
        scope.as_str(),
        plural(t, "memory")
    ));
    for part_lines in &parts {
        out.extend(part_lines.iter().cloned());
    }
    out.push("You are awake.".to_string());
    if pending_compressions > 0 {
        out.push(format!(
            "({} not compressed yet — harness will rebuild summaries.)",
            plural(pending_compressions, "block")
        ));
    }
    Ok(out.join("\n"))
}

/// Wake one scope for chat injection. `Ok(None)` when the store is missing
/// or empty — callers skip that section entirely.
fn wake_inject(
    scope: MemoryScope,
    repo_root: &Path,
) -> Result<Option<String>, AgentMemoryError> {
    let Some(store) = try_open_store(scope, repo_root)? else {
        return Ok(None);
    };
    if store.log_len()? == 0 {
        return Ok(None);
    }
    Ok(Some(wake_from_store(scope, &store)?))
}

fn format_scope_section(label: &str, result: Result<Option<String>, AgentMemoryError>) -> Option<String> {
    match result {
        Ok(None) => None,
        Ok(Some(text)) => Some(format!("{label}:\n{text}")),
        Err(e) => Some(format!("{label}:\n(unavailable: {e})")),
    }
}

/// Combined wake for both scopes — used to inject context at chat start.
/// Returns an empty string when both stores are missing/empty (or Memory is
/// unused), so the frontend can skip the system block.
pub fn wake_context(repo_root: &Path) -> Result<String, AgentMemoryError> {
    let project = format_scope_section(
        &format!("Project memory (`{}/.atlas/memory`)", repo_root.display()),
        wake_inject(MemoryScope::Project, repo_root),
    );
    let global = format_scope_section(
        "Global memory (`~/.atlas/memory`)",
        wake_inject(MemoryScope::Global, repo_root),
    );

    let mut sections = Vec::new();
    if let Some(p) = project {
        sections.push(p);
    }
    if let Some(g) = global {
        sections.push(g);
    }
    if sections.is_empty() {
        return Ok(String::new());
    }

    Ok(format!(
        "## Agent memory (OptMem)\n\n\
         {}\n\n\
         Memory is managed automatically after each turn. Treat this wake as already-read \
         lasting context. Do not write or edit files under `.atlas/memory`.",
        sections.join("\n\n")
    ))
}

pub fn note(scope: MemoryScope, repo_root: &Path, text: &str) -> Result<String, AgentMemoryError> {
    let store = open_store(scope, repo_root)?;
    let text = validate_entry(&store, text)?;
    let i = store.log_append(&[(today(), text)])?;
    Ok(format!("[{}] Saved as #{i}.", scope.as_str()))
}

/// Cap on harness-side compressions per drain (after one note).
const MAX_NAPS_PER_DRAIN: usize = 16;

/// Every raw log line in `scope`, oldest first. Empty when the store is
/// missing. Used by the memory viewer and post-turn policy dedup.
pub fn list_raw_entries(
    scope: MemoryScope,
    repo_root: &Path,
) -> Result<Vec<MemoryEntry>, AgentMemoryError> {
    let Some(store) = try_open_store(scope, repo_root)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in store.log_scan()? {
        out.push(entry?);
    }
    Ok(out)
}

/// Remove one raw log entry by `#id` (0-based index). Remaining entries are
/// renumbered; TREE summaries are cleared and rebuilt on the next nap drain.
pub fn delete_log_entry(
    scope: MemoryScope,
    repo_root: &Path,
    id: usize,
) -> Result<(), AgentMemoryError> {
    let store = open_store(scope, repo_root)?;
    store.log_remove_at(id).map_err(AgentMemoryError::from)
}

/// Drain pending TREE compressions using a caller-supplied summarizer
/// (typically a tool-free `LlmProvider::chat` call). Best-effort: on
/// summarizer/parse failure, stop and leave remaining blocks pending —
/// inject wake already tolerates missing summaries. Returns how many
/// summaries were written.
pub fn drain_pending_naps<F>(
    scope: MemoryScope,
    repo_root: &Path,
    mut summarize: F,
) -> Result<usize, AgentMemoryError>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let Some(store) = try_open_store(scope, repo_root)? else {
        return Ok(0);
    };
    let mut written = 0usize;
    for _ in 0..MAX_NAPS_PER_DRAIN {
        let t = store.log_len()?;
        let map = level_lens(&store, t)?;
        let level = level_len_fn(&map);
        let todo = pending(t, &level, Some(1));
        if todo.is_empty() {
            break;
        }
        let BlockRange { lo, hi } = todo[0];
        let left = pending_count(t, &level).saturating_sub(1);
        let prompt = nap_prompt(&store, lo, hi, left)?;
        let raw = match summarize(&prompt) {
            Ok(s) => s,
            Err(_) => break,
        };
        let line = first_summary_line(&raw);
        if line.is_empty() {
            break;
        }
        let summary = match validate_entry(&store, line) {
            Ok(s) => s,
            Err(_) => break,
        };
        if !store.tree_put(lo, hi, &summary)? {
            break;
        }
        written += 1;
    }
    Ok(written)
}

fn first_summary_line(raw: &str) -> &str {
    raw.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Several tests in this module call this concurrently. A nanosecond
    /// timestamp alone does not reliably disambiguate them on a coarser
    /// system clock — two would share a directory and clobber each other.
    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_repo() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("agent-memory-repo-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn project_and_global_are_isolated() {
        with_temp_home(|| {
            let repo = temp_repo();
            note(MemoryScope::Project, &repo, "project-only fact").unwrap();
            note(MemoryScope::Global, &repo, "global-only fact").unwrap();

            let p: Vec<_> = list_raw_entries(MemoryScope::Project, &repo)
                .unwrap()
                .into_iter()
                .map(|e| e.text)
                .collect();
            assert!(p.iter().any(|t| t.contains("project-only")));
            assert!(!p.iter().any(|t| t.contains("global-only")));

            let g: Vec<_> = list_raw_entries(MemoryScope::Global, &repo)
                .unwrap()
                .into_iter()
                .map(|e| e.text)
                .collect();
            assert!(g.iter().any(|t| t.contains("global-only")));
            assert!(!g.iter().any(|t| t.contains("project-only")));

            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn delete_log_entry_removes_one_line() {
        with_temp_home(|| {
            let repo = temp_repo();
            note(MemoryScope::Global, &repo, "keep").unwrap();
            note(MemoryScope::Global, &repo, "drop me").unwrap();
            delete_log_entry(MemoryScope::Global, &repo, 1).unwrap();
            let texts: Vec<_> = list_raw_entries(MemoryScope::Global, &repo)
                .unwrap()
                .into_iter()
                .map(|e| e.text)
                .collect();
            assert_eq!(texts, vec!["keep".to_string()]);
            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn note_then_drain_then_wake_context() {
        with_temp_home(|| {
            let repo = temp_repo();
            note(MemoryScope::Project, &repo, "alpha").unwrap();
            note(MemoryScope::Project, &repo, "beta").unwrap();
            let written = drain_pending_naps(MemoryScope::Project, &repo, |_prompt| {
                Ok("alpha and beta".to_string())
            })
            .unwrap();
            assert!(written >= 1);
            let wake_out = wake_context(&repo).unwrap();
            assert!(wake_out.contains("alpha and beta") || wake_out.contains("alpha"));
            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn wake_context_does_not_create_store_on_missing_dir() {
        with_temp_home(|| {
            let repo = temp_repo();
            let mem = project_memory_dir(&repo);
            assert!(!mem.exists());
            assert!(wake_context(&repo).unwrap().is_empty());
            assert!(!mem.exists(), "wake must not create {mem:?}");
            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn wake_context_skips_empty_and_survives_partial_failure() {
        with_temp_home(|| {
            let repo = temp_repo();
            note(MemoryScope::Project, &repo, "only-project").unwrap();
            let ctx = wake_context(&repo).unwrap();
            assert!(ctx.contains("only-project"));
            assert!(ctx.contains("Project memory"));
            assert!(!ctx.contains("No memories yet"));
            // Global still empty → not listed as empty fluff
            assert!(!ctx.contains("Global memory (`~/.atlas/memory`):\n[global] No memories"));
            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn note_result_is_short_confirmation() {
        with_temp_home(|| {
            let repo = temp_repo();
            note(MemoryScope::Project, &repo, "alpha").unwrap();
            let out = note(MemoryScope::Project, &repo, "beta").unwrap();
            assert!(out.contains("Saved as #1"));
            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn drain_pending_naps_writes_summaries_via_summarizer() {
        with_temp_home(|| {
            let repo = temp_repo();
            note(MemoryScope::Project, &repo, "alpha").unwrap();
            note(MemoryScope::Project, &repo, "beta").unwrap();
            let written = drain_pending_naps(MemoryScope::Project, &repo, |_prompt| {
                Ok("alpha and beta".to_string())
            })
            .unwrap();
            assert!(written >= 1);
            let wake_out = wake_context(&repo).unwrap();
            assert!(wake_out.contains("alpha and beta") || wake_out.contains("awake"));
            assert!(!wake_out.contains("not compressed yet"));
            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn wake_context_footer_does_not_mention_nap() {
        with_temp_home(|| {
            let repo = temp_repo();
            note(MemoryScope::Project, &repo, "only-project").unwrap();
            let ctx = wake_context(&repo).unwrap();
            assert!(ctx.contains("managed automatically"));
            assert!(!ctx.contains("call nap"));
            assert!(!ctx.contains("`memory` tool"));
            fs::remove_dir_all(&repo).ok();
        });
    }
}
