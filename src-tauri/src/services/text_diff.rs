//! Line-level text diffing for `services::ai_tools`'s file-mutating tools
//! (`write_file`/`edit_file`/`delete_file`) — one pure function computing
//! `domain::ai_tools::FileDiffStats`, consumed both by the chat UI (a
//! `+N -M` badge and colored diff view) and by the model itself (embedded
//! verbatim in the `ToolResult` it reads back). Split out from the
//! already-large `services::ai_tools` since this is an independent,
//! self-contained concern with nothing else in common with tool execution
//! (same rationale as `services::chunk_builder`/`services::repo_index`
//! living apart from it).

use similar::{ChangeTag, TextDiff};

use crate::domain::ai_tools::FileDiffStats;

/// Caps `FileDiffStats::unified_diff` so neither the model's context nor the
/// chat UI's detail view has to render an unbounded diff for a huge file —
/// `lines_added`/`lines_removed` stay exact regardless of this cap.
pub const MAX_UNIFIED_DIFF_CHARS: usize = 6000;

/// Computes `old` → `new`'s line diff. A brand-new file (nothing to diff
/// against) or a deleted file (nothing left to diff against) need no
/// special-casing here — callers just pass `""` for the side that doesn't
/// exist, and the diff naturally comes out as all-added/all-removed.
pub fn diff_stats(old: &str, new: &str) -> FileDiffStats {
    let diff = TextDiff::from_lines(old, new);

    let (mut lines_added, mut lines_removed) = (0u32, 0u32);
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => lines_added += 1,
            ChangeTag::Delete => lines_removed += 1,
            ChangeTag::Equal => {}
        }
    }

    let full = diff.unified_diff().context_radius(2).to_string();
    let (unified_diff, truncated) = truncate_on_line_boundary(&full, MAX_UNIFIED_DIFF_CHARS);

    FileDiffStats { lines_added, lines_removed, unified_diff, truncated }
}

/// Cuts `text` down to at most `max_chars`, but never mid-line — a diff cut
/// mid-line reads as corrupted rather than merely incomplete, and this text
/// is arbitrary UTF-8 document content, so a raw byte-index slice risks
/// panicking on a multi-byte character boundary. Keeps whole lines (with
/// their trailing `\n`) up to the point where the next one would cross
/// `max_chars`.
fn truncate_on_line_boundary(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    let mut kept = String::new();
    let mut kept_chars = 0usize;
    for line in text.split_inclusive('\n') {
        let line_chars = line.chars().count();
        if kept_chars + line_chars > max_chars {
            break;
        }
        kept.push_str(line);
        kept_chars += line_chars;
    }
    (kept, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_content_has_no_changes() {
        let stats = diff_stats("foo\nbar\n", "foo\nbar\n");
        assert_eq!(stats.lines_added, 0);
        assert_eq!(stats.lines_removed, 0);
        assert_eq!(stats.unified_diff, "");
        assert!(!stats.truncated);
    }

    #[test]
    fn new_file_reports_every_line_added() {
        let stats = diff_stats("", "foo\nbar\nbaz\n");
        assert_eq!(stats.lines_added, 3);
        assert_eq!(stats.lines_removed, 0);
        assert!(stats.unified_diff.contains("+foo"));
    }

    #[test]
    fn deleted_file_reports_every_line_removed() {
        let stats = diff_stats("foo\nbar\n", "");
        assert_eq!(stats.lines_added, 0);
        assert_eq!(stats.lines_removed, 2);
        assert!(stats.unified_diff.contains("-foo"));
    }

    #[test]
    fn mixed_change_counts_both_sides() {
        let stats = diff_stats("foo\nbar\nbaz\n", "foo\nqux\nbaz\n");
        assert_eq!(stats.lines_added, 1);
        assert_eq!(stats.lines_removed, 1);
    }

    #[test]
    fn a_large_diff_is_truncated_on_a_line_boundary() {
        let new: String = (0..2000).map(|i| format!("line {i}\n")).collect();
        let stats = diff_stats("", &new);
        assert!(stats.truncated);
        assert!(stats.unified_diff.len() <= MAX_UNIFIED_DIFF_CHARS);
        // Never cuts mid-line — the captured text always ends with a
        // complete line's trailing newline.
        assert!(stats.unified_diff.ends_with('\n'));
        // The true counts stay exact even though the rendered diff was cut.
        assert_eq!(stats.lines_added, 2000);
    }
}
