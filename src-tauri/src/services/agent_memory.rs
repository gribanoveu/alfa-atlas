//! Agent permanent memory — OptMem adapted for Atlas dual roots.
//!
//! - `project` → `{repoRoot}/.atlas/memory/`
//! - `global`  → `~/.atlas/memory/`

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::domain::optmem::{
    cover, paginate, parse_block_id, pending, pending_count, plural, BlockRange, MemoryEntry,
    OptMemError, OptMemKnobs, LOG_REC, RAW_MAX, TREE_REC,
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

fn next_nap(store: &OptMemStore, t: usize) -> Result<Option<String>, AgentMemoryError> {
    let map = level_lens(store, t)?;
    let level = level_len_fn(&map);
    let todo = pending(t, &level, Some(1));
    if todo.is_empty() {
        return Ok(None);
    }
    let BlockRange { lo, hi } = todo[0];
    let left = pending_count(t, &level).saturating_sub(1);
    Ok(Some(nap_prompt(store, lo, hi, left)?))
}

/// Prompt for harness-side compression (no tool-call instructions — the
/// summarizer returns one line that `drain_pending_naps` writes via `nap`).
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

pub fn wake(
    scope: MemoryScope,
    repo_root: &Path,
    part: Option<usize>,
    snapshot_t: Option<usize>,
) -> Result<String, AgentMemoryError> {
    let Some(store) = try_open_store(scope, repo_root)? else {
        return Ok(format!(
            "[{}] No memories yet. Record the first with op \"note\".\nYou are awake.",
            scope.as_str()
        ));
    };
    wake_from_store(scope, &store, part, snapshot_t, WakeMode::Tool)
}

#[derive(Clone, Copy)]
enum WakeMode {
    /// Full OptMem wake: may demand nap before returning cover lines.
    Tool,
    /// Chat auto-inject: never emit nap prompts; skip missing summaries.
    Inject,
}

fn wake_from_store(
    scope: MemoryScope,
    store: &OptMemStore,
    part: Option<usize>,
    snapshot_t: Option<usize>,
    mode: WakeMode,
) -> Result<String, AgentMemoryError> {
    let now = store.log_len()?;
    let k = part.unwrap_or(1);
    let t = snapshot_t.unwrap_or(now);
    if t > now {
        return Err(AgentMemoryError::Message(format!(
            "T={t}, but the log holds {}. Run wake without T.",
            plural(now, "memory")
        )));
    }
    if t == 0 {
        return Ok(format!(
            "[{}] No memories yet. Record the first with op \"note\".\nYou are awake.",
            scope.as_str()
        ));
    }

    let map = level_lens(store, t)?;
    let level = level_len_fn(&map);

    let mut lines = Vec::new();
    let mut pending_compressions = 0usize;
    for BlockRange { lo, hi } in cover(t, store.knobs().wake_lines) {
        if hi - lo == 1 {
            let e = store.log_get(lo)?;
            lines.push(format!("#{} {} {}", e.id, e.date, e.text));
        } else {
            let mut s = store.tree_get(lo, hi)?;
            if s.is_none() {
                match mode {
                    WakeMode::Inject => {
                        pending_compressions += 1;
                        lines.push(format!(
                            "#{}-{} (not compressed yet)",
                            lo,
                            hi - 1
                        ));
                        continue;
                    }
                    WakeMode::Tool => {
                        if let Some(nap) = next_nap(store, t)? {
                            return Ok(format!(
                                "[{}] Cannot wake: the memory context needs #{}-{}, which is not compressed yet.\n\
                                 Do the {} below, then run wake again.\n\n{nap}",
                                scope.as_str(),
                                lo,
                                hi - 1,
                                plural(pending_count(t, &level), "compression"),
                            ));
                        }
                        s = store.tree_get(lo, hi)?;
                        if s.is_none() {
                            return Err(AgentMemoryError::Store(OptMemStoreError::BlankSummary {
                                lo,
                                hi: hi - 1,
                            }));
                        }
                    }
                }
            }
            lines.push(format!("#{}-{} {}", lo, hi - 1, s.unwrap()));
        }
    }

    let parts = paginate(
        &lines,
        store.knobs().part_chars,
        store.knobs().part_lines,
    );

    // Inject mode concatenates every part so the model never needs wake
    // pagination. Tool mode (internal / tests) still paginates.
    match mode {
        WakeMode::Inject => {
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
        WakeMode::Tool => {
            if !(1..=parts.len()).contains(&k) {
                return Err(AgentMemoryError::Message(format!(
                    "No part {k}: the memory has {}. Run wake.",
                    plural(parts.len(), "part")
                )));
            }

            let mut out = Vec::new();
            if parts.len() > 1 {
                out.push(format!(
                    "[{}] Your memory, part {k} of {}, oldest first ({}).",
                    scope.as_str(),
                    parts.len(),
                    plural(t, "memory")
                ));
            } else {
                out.push(format!("[{}] Your memory ({}):", scope.as_str(), plural(t, "memory")));
            }
            out.extend(parts[k - 1].iter().cloned());
            if k < parts.len() {
                out.push(format!(
                    "Not awake yet. Call memory op \"wake\" with part={} and snapshotT={t}.",
                    k + 1
                ));
            } else {
                out.push("You are awake.".to_string());
                if let Some(nap) = next_nap(store, t)? {
                    out.push(String::new());
                    out.push(nap);
                }
            }
            Ok(out.join("\n"))
        }
    }
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
    Ok(Some(wake_from_store(
        scope,
        &store,
        None,
        None,
        WakeMode::Inject,
    )?))
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
    // Pending TREE compressions are drained by the harness (`drain_pending_naps`)
    // after this tool returns — do not ask the model to call `nap`.
    Ok(format!("[{}] Saved as #{i}.", scope.as_str()))
}

pub fn nap(
    scope: MemoryScope,
    repo_root: &Path,
    block: Option<&str>,
    text: Option<&str>,
) -> Result<String, AgentMemoryError> {
    let Some(store) = try_open_store(scope, repo_root)? else {
        return Ok(format!("[{}] Nothing left to compress.", scope.as_str()));
    };
    let t = store.log_len()?;
    let map = level_lens(&store, t)?;
    let level = level_len_fn(&map);
    let mut said = false;
    let mut out = String::new();

    if let (Some(block), Some(text)) = (block, text) {
        said = true;
        let BlockRange { lo, hi } = parse_block_id(block)?;
        let todo = pending(t, &level, Some(1));
        if todo.is_empty() {
            return Ok(format!("[{}] Nothing left to compress.", scope.as_str()));
        }
        if (lo, hi) != (todo[0].lo, todo[0].hi) {
            if store.tree_get(lo, hi)?.is_some() {
                out.push_str(&format!(
                    "[{}] {}-{} is already settled.",
                    scope.as_str(),
                    lo,
                    hi - 1
                ));
            } else {
                return Err(AgentMemoryError::Message(format!(
                    "Wrong block: {block}. Blocks are built in order; the next is {}-{}. Call nap without args or with that block.",
                    todo[0].lo,
                    todo[0].hi - 1
                )));
            }
        } else {
            let summary = validate_entry(&store, text)?;
            if !store.tree_put(lo, hi, &summary)? {
                out.push_str(&format!(
                    "[{}] {}-{} was settled or forgotten meanwhile.",
                    scope.as_str(),
                    lo,
                    hi - 1
                ));
            } else {
                out.push_str(&format!(
                    "[{}] {}-{} saved.",
                    scope.as_str(),
                    lo,
                    hi - 1
                ));
            }
        }
    }

    match next_nap(&store, t)? {
        None => {
            if out.is_empty() {
                Ok(format!("[{}] Nothing left to compress.", scope.as_str()))
            } else {
                out.push_str("\nNothing left to compress.");
                Ok(out)
            }
        }
        Some(nap) => {
            if said && !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&nap);
            Ok(out)
        }
    }
}

const RECALL_PATTERN_MAX: usize = 200;

pub fn recall(scope: MemoryScope, repo_root: &Path, pattern: &str) -> Result<String, AgentMemoryError> {
    if pattern.is_empty() {
        return Err(AgentMemoryError::Message(
            "op \"recall\" requires a non-empty `pattern`".into(),
        ));
    }
    if pattern.len() > RECALL_PATTERN_MAX {
        return Err(AgentMemoryError::Message(format!(
            "recall pattern is too long (max {RECALL_PATTERN_MAX} bytes)"
        )));
    }
    let Some(store) = try_open_store(scope, repo_root)? else {
        return Ok(format!("[{}] No match.", scope.as_str()));
    };
    let pat = regex::RegexBuilder::new(&format!("(?i){pattern}"))
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .map_err(|e| OptMemError::BadRegex(e.to_string()))?;
    let part_chars = store.knobs().part_chars;
    let mut hits = 0usize;
    let mut out: VecDeque<String> = VecDeque::new();
    let mut size = 0usize;
    for entry in store.log_scan()? {
        let e: MemoryEntry = entry?;
        let line = format!("#{} {} {}", e.id, e.date, e.text);
        if !pat.is_match(&line) {
            continue;
        }
        hits += 1;
        size += line.len() + 1;
        out.push_back(line);
        while size > part_chars {
            if let Some(front) = out.pop_front() {
                size -= front.len() + 1;
            } else {
                break;
            }
        }
    }
    if hits == 0 {
        return Ok(format!("[{}] No match.", scope.as_str()));
    }
    let mut text = format!("[{}]\n", scope.as_str());
    text.push_str(&out.iter().cloned().collect::<Vec<_>>().join("\n"));
    text.push('\n');
    if out.len() < hits {
        text.push_str(&format!(
            "Newest {} of {}. Narrow the regex.",
            out.len(),
            plural(hits, "match")
        ));
    } else {
        text.push_str(&format!("{}.", plural(hits, "match")));
    }
    Ok(text)
}

pub fn zoom(scope: MemoryScope, repo_root: &Path, block: &str) -> Result<String, AgentMemoryError> {
    let Some(store) = try_open_store(scope, repo_root)? else {
        return Err(AgentMemoryError::Message(format!(
            "#{block} is beyond the memory: it holds {}. Run wake.",
            plural(0, "memory")
        )));
    };
    let BlockRange { lo, hi } = parse_block_id(block)?;
    let t = store.log_len()?;
    if lo >= t {
        return Err(AgentMemoryError::Message(format!(
            "#{block} is beyond the memory: it holds {}. Run wake.",
            plural(t, "memory")
        )));
    }
    let mid = (lo + hi) / 2;
    let mut lines = vec![format!("[{}]", scope.as_str())];
    for (a, b) in [(lo, mid), (mid, hi)] {
        if a >= t {
            continue;
        }
        if b - a == 1 {
            let e = store.log_get(a)?;
            lines.push(format!("#{} {} {}", e.id, e.date, e.text));
        } else {
            let s = store
                .tree_get(a, b)?
                .unwrap_or_else(|| "not compressed yet".to_string());
            lines.push(format!("#{}-{} {}", a, b - 1, s));
        }
    }
    Ok(lines.join("\n"))
}

pub fn forget(scope: MemoryScope, repo_root: &Path, block: &str) -> Result<String, AgentMemoryError> {
    let Some(store) = try_open_store(scope, repo_root)? else {
        return Err(AgentMemoryError::Message(format!(
            "No summary at {block}."
        )));
    };
    let range = parse_block_id(block)?;
    let gone = store.tree_drop(range.lo, range.hi)?;
    if gone.is_empty() {
        return Err(AgentMemoryError::Message(format!(
            "No summary at {block}."
        )));
    }
    Ok(format!(
        "[{}] Forgot {}, from {}-{} up. Harness will rebuild summaries.",
        scope.as_str(),
        plural(gone.len(), "summary"),
        gone[0].lo,
        gone[0].hi - 1
    ))
}

pub fn config(
    scope: MemoryScope,
    repo_root: &Path,
    knob: Option<&str>,
) -> Result<String, AgentMemoryError> {
    let mut store = open_store(scope, repo_root)?;
    let mut over = store.overrides()?;
    if let Some(a) = knob {
        let Some((k_raw, v)) = a.split_once('=') else {
            return Err(AgentMemoryError::Message(format!(
                "usage: config NAME=VALUE (NAME one of {})",
                crate::domain::optmem::KNOB_NAMES.join(", ")
            )));
        };
        let k = k_raw.trim().to_uppercase();
        let v = v.trim();
        if !crate::domain::optmem::KNOB_NAMES.contains(&k.as_str()) {
            return Err(OptMemError::UnknownKnob(k).into());
        }
        if v.is_empty() {
            over.remove(&k);
        } else {
            let parsed: usize = v.parse().map_err(|_| OptMemError::InvalidKnobValue {
                name: k.clone(),
                value: v.to_string(),
            })?;
            validate_knob(&k, parsed)?;
            over.insert(k, parsed);
        }
        store.set_overrides(&over)?;
    }
    let defaults = OptMemKnobs::default();
    let mut lines = vec![format!("[{}] sizes:", scope.as_str())];
    for (name, default, what) in [
        ("WAKE_LINES", defaults.wake_lines, "the memory context: how many lines wake prints"),
        ("ENTRY_CHARS", defaults.entry_chars, "the longest one memory may be, in bytes"),
        ("PART_CHARS", defaults.part_chars, "output paging: largest part, in bytes"),
        ("PART_LINES", defaults.part_lines, "output paging: largest part, in lines"),
    ] {
        let val = over.get(name).copied().unwrap_or(default);
        let mark = if over.contains_key(name) {
            format!(" (default {default})")
        } else {
            String::new()
        };
        lines.push(format!("{name:<12} {val:<7} {what}{mark}"));
    }
    Ok(lines.join("\n"))
}

fn validate_knob(name: &str, value: usize) -> Result<(), AgentMemoryError> {
    OptMemKnobs::validate_positive(name, value)?;
    if name == "ENTRY_CHARS" {
        let top = (TREE_REC - 8).min(LOG_REC - 40);
        if value > top {
            return Err(OptMemError::EntryCharsTooLarge { max: top }.into());
        }
    }
    crate::domain::optmem::check_knob_max(name, value)?;
    Ok(())
}

/// Cap on harness-side compressions per drain (after one note/forget).
const MAX_NAPS_PER_DRAIN: usize = 16;

/// Every raw log line in `scope`, oldest first. Empty when the store is
/// missing. Used by the post-turn memory policy for dedup / supersede —
/// not a model-facing op.
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

/// Dispatch a model-facing memory op. Wake/nap/zoom/config are harness-managed
/// and rejected here with a clear error — use the internal functions instead.
pub fn run_op(
    scope: MemoryScope,
    repo_root: &Path,
    op: &str,
    text: Option<&str>,
    pattern: Option<&str>,
    block: Option<&str>,
    _knob: Option<&str>,
    _part: Option<usize>,
    _snapshot_t: Option<usize>,
) -> Result<String, AgentMemoryError> {
    match op {
        "wake" | "nap" | "zoom" | "config" => Err(AgentMemoryError::Message(format!(
            "op \"{op}\" is harness-managed; use note, recall, or forget"
        ))),
        "note" => {
            let text = text.ok_or_else(|| {
                AgentMemoryError::Message("op \"note\" requires `text`".into())
            })?;
            note(scope, repo_root, text)
        }
        "recall" => {
            let pattern = pattern.ok_or_else(|| {
                AgentMemoryError::Message("op \"recall\" requires `pattern`".into())
            })?;
            recall(scope, repo_root, pattern)
        }
        "forget" => {
            let block = block.ok_or_else(|| {
                AgentMemoryError::Message("op \"forget\" requires `block`".into())
            })?;
            forget(scope, repo_root, block)
        }
        other => Err(AgentMemoryError::Message(format!(
            "unknown memory op: \"{other}\" (expected note|recall|forget)"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("agent-memory-repo-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn project_and_global_are_isolated() {
        with_temp_home(|| {
            let repo = temp_repo();
            note(MemoryScope::Project, &repo, "project-only fact").unwrap();
            note(MemoryScope::Global, &repo, "global-only fact").unwrap();

            let p = recall(MemoryScope::Project, &repo, "project-only").unwrap();
            assert!(p.contains("project-only"));
            assert!(!p.contains("global-only"));

            let g = recall(MemoryScope::Global, &repo, "global-only").unwrap();
            assert!(g.contains("global-only"));
            assert!(!g.contains("project-only"));

            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn note_then_nap_then_wake() {
        with_temp_home(|| {
            let repo = temp_repo();
            let out = note(MemoryScope::Project, &repo, "alpha").unwrap();
            assert!(out.contains("Saved as #0"));
            note(MemoryScope::Project, &repo, "beta").unwrap();
            // size-2 block should be pending
            let nap_out = nap(
                MemoryScope::Project,
                &repo,
                Some("0-1"),
                Some("alpha and beta"),
            )
            .unwrap();
            assert!(nap_out.contains("0-1 saved") || nap_out.contains("Nothing left"));
            let wake_out = wake(MemoryScope::Project, &repo, None, None).unwrap();
            assert!(wake_out.contains("awake") || wake_out.contains("alpha"));
            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn config_rejects_wake_lines_above_the_cap() {
        use crate::domain::optmem::WAKE_LINES_MAX;

        with_temp_home(|| {
            let repo = temp_repo();
            let too_large = format!("WAKE_LINES={}", WAKE_LINES_MAX + 1);
            let err = config(MemoryScope::Project, &repo, Some(&too_large)).unwrap_err();
            assert!(matches!(
                err,
                AgentMemoryError::Domain(OptMemError::KnobTooLarge { .. })
            ));

            let at_cap = format!("WAKE_LINES={WAKE_LINES_MAX}");
            assert!(config(MemoryScope::Project, &repo, Some(&at_cap)).is_ok());

            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn wake_does_not_create_store_on_missing_dir() {
        with_temp_home(|| {
            let repo = temp_repo();
            let mem = project_memory_dir(&repo);
            assert!(!mem.exists());
            let out = wake(MemoryScope::Project, &repo, None, None).unwrap();
            assert!(out.contains("No memories yet"));
            assert!(!mem.exists(), "wake must not create {mem:?}");
            assert!(wake_context(&repo).unwrap().is_empty());
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
    fn recall_rejects_oversized_pattern() {
        with_temp_home(|| {
            let repo = temp_repo();
            let long = "a".repeat(RECALL_PATTERN_MAX + 1);
            let err = recall(MemoryScope::Project, &repo, &long).unwrap_err();
            assert!(matches!(err, AgentMemoryError::Message(_)));
            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn run_op_rejects_harness_managed_ops() {
        with_temp_home(|| {
            let repo = temp_repo();
            for op in ["wake", "nap", "zoom", "config"] {
                let err = run_op(
                    MemoryScope::Project,
                    &repo,
                    op,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap_err();
                let msg = err.to_string();
                assert!(
                    msg.contains("harness-managed"),
                    "op {op}: {msg}"
                );
            }
            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn note_result_does_not_ask_model_to_nap() {
        with_temp_home(|| {
            let repo = temp_repo();
            note(MemoryScope::Project, &repo, "alpha").unwrap();
            let out = note(MemoryScope::Project, &repo, "beta").unwrap();
            assert!(out.contains("Saved as #1"));
            assert!(!out.contains("Compress"));
            assert!(!out.contains("op \"nap\""));
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
