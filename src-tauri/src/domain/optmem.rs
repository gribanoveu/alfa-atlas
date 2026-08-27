//! OptMem algorithm — pure types and rules, no I/O.
//!
//! Port of VictorTaelin/OptMem's cover/pending/block-id/entry-validation
//! logic. Storage lives in `infra::optmem_store`; orchestration in
//! `services::agent_memory`.

use std::collections::BTreeMap;

use thiserror::Error;

/// Fixed-width log record size (bytes), including the trailing newline.
/// Wider than upstream OptMem's 320 so a one-line Russian note (~2 bytes
/// per Cyrillic letter) can still hold a useful project fact without three
/// compression retries.
pub const LOG_REC: usize = 640;
/// Fixed-width tree summary record size (bytes), including the trailing newline.
pub const TREE_REC: usize = 608;
/// Blocks up to this many raw memories compress from the raw log.
pub const RAW_MAX: usize = 16;

/// Default knobs — overridable per store via its `config` file.
pub const DEFAULT_WAKE_LINES: usize = 96;
/// Default max UTF-8 **bytes** per note/nap line (not Unicode scalars).
/// ~560 bytes ≈ 280 Latin chars or ~180 Cyrillic chars.
pub const DEFAULT_ENTRY_CHARS: usize = 560;
pub const DEFAULT_PART_CHARS: usize = 20_000;
pub const DEFAULT_PART_LINES: usize = 500;

/// Upper bounds for the knobs with no structural ceiling of their own
/// (unlike `ENTRY_CHARS`, which is capped by the fixed record width — see
/// its own check below). Without these, setting `WAKE_LINES` above a
/// store's total memory count disables OptMem's compression entirely:
/// `cover(t, budget)` only coarsens once `t > budget`, so every `wake`
/// would dump every raw entry, forever growing the block auto-injected into
/// the system prompt every turn as the log grows. `PART_CHARS`/`PART_LINES`
/// are capped for the same "bound one response" reason.
pub const WAKE_LINES_MAX: usize = 300;
pub const PART_CHARS_MAX: usize = 200_000;
pub const PART_LINES_MAX: usize = 5_000;

/// Names a store may override in its `config` file.
pub const KNOB_NAMES: &[&str] = &["WAKE_LINES", "ENTRY_CHARS", "PART_CHARS", "PART_LINES"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptMemKnobs {
    pub wake_lines: usize,
    pub entry_chars: usize,
    pub part_chars: usize,
    pub part_lines: usize,
}

impl Default for OptMemKnobs {
    fn default() -> Self {
        Self {
            wake_lines: DEFAULT_WAKE_LINES,
            entry_chars: DEFAULT_ENTRY_CHARS,
            part_chars: DEFAULT_PART_CHARS,
            part_lines: DEFAULT_PART_LINES,
        }
    }
}

impl OptMemKnobs {
    /// Merge overrides on top of defaults. Unknown keys are rejected by the
    /// caller before they reach here.
    pub fn with_overrides(overrides: &BTreeMap<String, usize>) -> Result<Self, OptMemError> {
        let mut knobs = Self::default();
        for (k, v) in overrides {
            match k.as_str() {
                "WAKE_LINES" => {
                    check_knob_max(k, *v)?;
                    knobs.wake_lines = *v;
                }
                "ENTRY_CHARS" => {
                    let top = (TREE_REC - 8).min(LOG_REC - 40);
                    if *v > top {
                        return Err(OptMemError::EntryCharsTooLarge { max: top });
                    }
                    knobs.entry_chars = *v;
                }
                "PART_CHARS" => {
                    check_knob_max(k, *v)?;
                    knobs.part_chars = *v;
                }
                "PART_LINES" => {
                    check_knob_max(k, *v)?;
                    knobs.part_lines = *v;
                }
                other => return Err(OptMemError::UnknownKnob(other.to_string())),
            }
        }
        Ok(knobs)
    }

    pub fn validate_positive(name: &str, value: usize) -> Result<usize, OptMemError> {
        if value < 1 {
            return Err(OptMemError::InvalidKnobValue {
                name: name.to_string(),
                value: value.to_string(),
            });
        }
        Ok(value)
    }
}

/// Upper-bound check for the knobs capped by `WAKE_LINES_MAX`/`PART_CHARS_MAX`/
/// `PART_LINES_MAX` — a no-op for `ENTRY_CHARS` (structural cap, checked
/// separately at each of its own call sites) and any unrecognized name.
/// Shared so the three enforcement sites (`OptMemKnobs::with_overrides`,
/// `infra::optmem_store::read_overrides`, `services::agent_memory::validate_knob`)
/// can't drift on the actual cap value.
pub fn check_knob_max(name: &str, value: usize) -> Result<(), OptMemError> {
    let max = match name {
        "WAKE_LINES" => WAKE_LINES_MAX,
        "PART_CHARS" => PART_CHARS_MAX,
        "PART_LINES" => PART_LINES_MAX,
        _ => return Ok(()),
    };
    if value > max {
        return Err(OptMemError::KnobTooLarge {
            name: name.to_string(),
            max,
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryEntry {
    pub id: usize,
    pub date: String,
    pub text: String,
}

/// Half-open block `[lo, hi)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRange {
    pub lo: usize,
    pub hi: usize,
}

impl BlockRange {
    pub fn size(self) -> usize {
        self.hi - self.lo
    }

}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OptMemError {
    #[error("empty. A memory is one line of text.")]
    EmptyEntry,
    #[error("{0} lines. A memory is one line: merge them, or note them separately.")]
    MultiLine(usize),
    #[error(
        "too long: {bytes} bytes, limit {limit} (~{cyrillic_budget} Cyrillic chars). \
         UTF-8: accented/Cyrillic letters cost 2+ bytes each. Compress it further."
    )]
    EntryTooLong {
        bytes: usize,
        limit: usize,
        cyrillic_budget: usize,
    },
    #[error("{name} must be a positive whole number, not '{value}'.")]
    InvalidKnobValue { name: String, value: String },
    #[error("ENTRY_CHARS is at most {max}: a memory has to fit the fixed-width records.")]
    EntryCharsTooLarge { max: usize },
    #[error("{name} is at most {max}: keeps one wake or response bounded.")]
    KnobTooLarge { name: String, max: usize },
    #[error("{0} is not a size. Name one of: WAKE_LINES, ENTRY_CHARS, PART_CHARS, PART_LINES.")]
    UnknownKnob(String),
}

/// Validate one memory line against `entry_chars` (byte length of UTF-8).
pub fn check_entry(text: &str, entry_chars: usize) -> Result<String, OptMemError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(OptMemError::EmptyEntry);
    }
    if text.contains('\n') || text.contains('\r') {
        return Err(OptMemError::MultiLine(text.lines().count().max(1)));
    }
    let n = text.len();
    if n > entry_chars {
        return Err(OptMemError::EntryTooLong {
            bytes: n,
            limit: entry_chars,
            cyrillic_budget: entry_chars / 2,
        });
    }
    Ok(text.to_string())
}

fn cover_alpha(t: usize, alpha: f64) -> Vec<BlockRange> {
    let mut root = 1usize;
    while root < t {
        root *= 2;
    }
    let mut out = Vec::new();
    let mut stack = vec![(0usize, root)];
    while let Some((lo, hi)) = stack.pop() {
        if lo >= t {
            continue;
        }
        let size = hi - lo;
        if size > 1 && (hi > t || (size as f64) > alpha * ((t - lo) as f64)) {
            let mid = (lo + hi) / 2;
            stack.push((mid, hi));
            stack.push((lo, mid));
        } else {
            out.push(BlockRange { lo, hi });
        }
    }
    out.sort_by_key(|b| b.lo);
    out
}

/// The blocks `wake` prints: at most `budget` of them, finest near `t`.
pub fn cover(t: usize, budget: usize) -> Vec<BlockRange> {
    if t == 0 {
        return Vec::new();
    }
    if t <= budget {
        return (0..t).map(|i| BlockRange { lo: i, hi: i + 1 }).collect();
    }
    let mut lo = 0.0_f64;
    let mut hi = 1.0_f64;
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if cover_alpha(t, mid).len() > budget {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let mut out = cover_alpha(t, hi);
    while out.len() < budget {
        let Some(i) = out
            .iter()
            .enumerate()
            .filter(|(_, b)| b.size() > 1)
            .map(|(i, _)| i)
            .max()
        else {
            break;
        };
        let BlockRange { lo: lo_, hi: hi_ } = out[i];
        let mid = (lo_ + hi_) / 2;
        out.splice(i..=i, [BlockRange { lo: lo_, hi: mid }, BlockRange { lo: mid, hi: hi_ }]);
    }
    out
}

/// Blocks that can be built and have not been, smallest first.
///
/// `level_len(size)` returns how many summaries already exist at that power-
/// of-two level (from the store's TREE file length).
pub fn pending(
    t: usize,
    level_len: &dyn Fn(usize) -> usize,
    limit: Option<usize>,
) -> Vec<BlockRange> {
    let mut todo = Vec::new();
    let mut size = 2usize;
    while size <= t {
        let have = level_len(size);
        for k in have..(t / size) {
            todo.push(BlockRange {
                lo: k * size,
                hi: (k + 1) * size,
            });
            if limit.is_some_and(|lim| todo.len() >= lim) {
                return todo;
            }
        }
        size *= 2;
    }
    todo
}

pub fn pending_count(t: usize, level_len: &dyn Fn(usize) -> usize) -> usize {
    let mut n = 0usize;
    let mut size = 2usize;
    while size <= t {
        n += (t / size).saturating_sub(level_len(size));
        size *= 2;
    }
    n
}

/// Split lines into parts that fit PART_CHARS / PART_LINES caps.
pub fn paginate(lines: &[String], part_chars: usize, part_lines: usize) -> Vec<Vec<String>> {
    let mut parts: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut size = 0usize;
    for line in lines {
        let n = line.len() + 1;
        if !cur.is_empty() && (cur.len() >= part_lines || size + n > part_chars) {
            parts.push(std::mem::take(&mut cur));
            size = 0;
        }
        cur.push(line.clone());
        size += n;
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

pub fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        return format!("1 {word}");
    }
    let pluralized = if word.ends_with('y') && !word.ends_with("ay") && !word.ends_with("ey") {
        format!("{}ies", &word[..word.len() - 1])
    } else if word.ends_with(['s', 'h', 'x']) {
        format!("{word}es")
    } else {
        format!("{word}s")
    };
    format!("{n} {pluralized}")
}

/// Format a padded fixed-width record (LOG_REC or TREE_REC).
pub fn pad_record(text: &str, rec: usize) -> Result<Vec<u8>, OptMemError> {
    let b = text.as_bytes();
    if b.len() > rec - 1 {
        let limit = rec - 1;
        return Err(OptMemError::EntryTooLong {
            bytes: b.len(),
            limit,
            cyrillic_budget: limit / 2,
        });
    }
    let mut out = Vec::with_capacity(rec);
    out.extend_from_slice(b);
    out.resize(rec - 1, b' ');
    out.push(b'\n');
    Ok(out)
}

/// Parse one log line `#id YYYY-MM-DD text`.
pub fn parse_log_line(line: &str) -> Option<MemoryEntry> {
    let line = line.trim_end();
    let rest = line.strip_prefix('#')?;
    let (id_s, rest) = rest.split_once(' ')?;
    let id: usize = id_s.parse().ok()?;
    let (date, text) = rest.split_once(' ')?;
    Some(MemoryEntry {
        id,
        date: date.to_string(),
        text: text.trim_end().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_small_t_is_singletons() {
        let c = cover(3, 96);
        assert_eq!(
            c,
            vec![
                BlockRange { lo: 0, hi: 1 },
                BlockRange { lo: 1, hi: 2 },
                BlockRange { lo: 2, hi: 3 },
            ]
        );
    }

    #[test]
    fn cover_respects_budget() {
        let c = cover(100, 10);
        assert!(c.len() <= 10);
        assert_eq!(c.first().map(|b| b.lo), Some(0));
        assert_eq!(c.last().map(|b| b.hi), Some(100));
    }

    #[test]
    fn with_overrides_rejects_knobs_above_their_cap() {
        let over = |k: &str, v: usize| BTreeMap::from([(k.to_string(), v)]);
        assert!(matches!(
            OptMemKnobs::with_overrides(&over("WAKE_LINES", WAKE_LINES_MAX + 1)),
            Err(OptMemError::KnobTooLarge { .. })
        ));
        assert!(matches!(
            OptMemKnobs::with_overrides(&over("PART_CHARS", PART_CHARS_MAX + 1)),
            Err(OptMemError::KnobTooLarge { .. })
        ));
        assert!(matches!(
            OptMemKnobs::with_overrides(&over("PART_LINES", PART_LINES_MAX + 1)),
            Err(OptMemError::KnobTooLarge { .. })
        ));
        assert!(OptMemKnobs::with_overrides(&over("WAKE_LINES", WAKE_LINES_MAX)).is_ok());
    }

    #[test]
    fn check_knob_max_is_a_noop_for_entry_chars_and_unknown_names() {
        assert!(check_knob_max("ENTRY_CHARS", usize::MAX).is_ok());
        assert!(check_knob_max("SOMETHING_ELSE", usize::MAX).is_ok());
    }

    #[test]
    fn check_entry_enforces_byte_limit() {
        assert!(check_entry("ok", DEFAULT_ENTRY_CHARS).is_ok());
        assert!(matches!(
            check_entry("", DEFAULT_ENTRY_CHARS),
            Err(OptMemError::EmptyEntry)
        ));
        assert!(matches!(
            check_entry("a\nb", DEFAULT_ENTRY_CHARS),
            Err(OptMemError::MultiLine(_))
        ));
        let long = "x".repeat(DEFAULT_ENTRY_CHARS + 1);
        assert!(matches!(
            check_entry(&long, DEFAULT_ENTRY_CHARS),
            Err(OptMemError::EntryTooLong { .. })
        ));
        // A dense Russian one-liner that used to blow the 280-byte upstream
        // default must fit the Cyrillic-friendly Atlas default.
        let ru = "corp-wowtax-patent-notification-api: микросервис Альфа-Банка для уведомлений о патентном налоге (ПСН/wowtax). Java 21, Spring, MongoDB, Kafka; интеграции MKS, wowtax, corp-sign; PDF/XML через pdfbox. REST-доки в src/docs/asciidoc.";
        assert!(
            ru.len() > 280,
            "fixture should exceed the old 280-byte cap (got {} bytes)",
            ru.len()
        );
        assert!(check_entry(ru, DEFAULT_ENTRY_CHARS).is_ok());
    }

    #[test]
    fn pending_lists_missing_size_2_first() {
        let level = |size: usize| if size == 2 { 1 } else { 0 };
        let p = pending(4, &level, None);
        assert_eq!(p[0], BlockRange { lo: 2, hi: 4 });
    }

    #[test]
    fn pad_record_is_fixed_width() {
        let r = pad_record("hi", 16).unwrap();
        assert_eq!(r.len(), 16);
        assert_eq!(r[15], b'\n');
    }

    #[test]
    fn plural_basic() {
        assert_eq!(plural(1, "memory"), "1 memory");
        assert_eq!(plural(2, "memory"), "2 memories");
        assert_eq!(plural(2, "match"), "2 matches");
        assert_eq!(plural(2, "summary"), "2 summaries");
    }
}
