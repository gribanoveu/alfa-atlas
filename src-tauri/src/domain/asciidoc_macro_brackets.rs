//! Closes bare AsciiDoc block macros with empty attribute brackets.
//!
//! `include::path.adoc`, `image::path.png`, and `xref:doc.adoc[#anchor]` are
//! not valid without a trailing `[…]`. This pass is idempotent and skips
//! listing / literal / comment / passthrough fences so sample markup is left
//! alone.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

static INCLUDE_IMAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(include|image)::([^\s\[]+)").expect("valid regex"));
static XREF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bxref:([^\s\[]+)").expect("valid regex"));

/// One macro this pass rewrote.
///
/// Reported rather than merely counted because the caller of interest is the
/// assistant, and "your line 12 is now `include::request.adoc[]`" is
/// actionable in a way "3 macros were fixed" is not — it has to be able to
/// reconcile what it sent with what is on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosedMacro {
    /// 1-indexed line in the content that was written.
    pub line: u32,
    /// The macro as it now reads, brackets included.
    pub text: String,
}

/// Result of one pass: the corrected content, plus what it had to correct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroBracketPass {
    pub content: String,
    /// Empty when the content was already valid — the overwhelmingly common
    /// case, and the one where callers should say nothing at all.
    pub closed: Vec<ClosedMacro>,
}

/// Append `[]` to bare `include::` / `image::` / `xref:` targets in `content`.
pub fn ensure_macro_attribute_brackets(content: &str) -> MacroBracketPass {
    if content.is_empty() {
        return MacroBracketPass { content: String::new(), closed: vec![] };
    }
    let newline = if content.contains("\r\n") { "\r\n" } else { "\n" };
    let ends_with_newline = content.ends_with('\n');
    let mut in_fence: Option<char> = None;
    let mut out: Vec<String> = Vec::new();
    let mut closed: Vec<ClosedMacro> = Vec::new();

    for (index, line) in content.lines().enumerate() {
        if let Some(kind) = in_fence {
            if is_fence_line(line, Some(kind)) {
                in_fence = None;
            }
            out.push(line.to_string());
            continue;
        }
        if let Some(kind) = fence_open(line) {
            in_fence = Some(kind);
            out.push(line.to_string());
            continue;
        }
        let (fixed, on_this_line) = close_macros_on_line(line);
        for text in on_this_line {
            closed.push(ClosedMacro { line: index as u32 + 1, text });
        }
        out.push(fixed);
    }

    let mut result = out.join(newline);
    if ends_with_newline {
        result.push_str(newline);
    }
    MacroBracketPass { content: result, closed }
}

/// The fixed line, plus each macro on it that needed fixing (in the form it
/// now takes).
fn close_macros_on_line(line: &str) -> (String, Vec<String>) {
    let mut closed: Vec<String> = Vec::new();
    let with_includes = INCLUDE_IMAGE_RE.replace_all(line, |caps: &regex::Captures| {
        let whole = caps.get(0).expect("full match");
        let target = &caps[2];
        if skip_target(target) || line[whole.end()..].starts_with('[') {
            whole.as_str().to_string()
        } else {
            let fixed = format!("{}::{}[]", &caps[1], target);
            closed.push(fixed.clone());
            fixed
        }
    });
    let result = XREF_RE
        .replace_all(&with_includes, |caps: &regex::Captures| {
            let whole = caps.get(0).expect("full match");
            let target = &caps[1];
            if skip_target(target) || with_includes[whole.end()..].starts_with('[') {
                whole.as_str().to_string()
            } else {
                let fixed = format!("xref:{target}[]");
                closed.push(fixed.clone());
                fixed
            }
        })
        .into_owned();
    (result, closed)
}

fn skip_target(target: &str) -> bool {
    target.is_empty() || target.ends_with('/') || target.ends_with('#') || target.ends_with(':')
}

fn fence_open(line: &str) -> Option<char> {
    fence_kind(line.trim())
}

fn is_fence_line(line: &str, expected: Option<char>) -> bool {
    match (fence_kind(line.trim()), expected) {
        (Some(kind), Some(expected)) => kind == expected,
        (Some(_), None) => true,
        _ => false,
    }
}

fn fence_kind(trimmed: &str) -> Option<char> {
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if !matches!(first, '-' | '.' | '/' | '+') {
        return None;
    }
    let mut count = 1;
    for c in chars {
        if c != first {
            return None;
        }
        count += 1;
    }
    if count >= 4 {
        Some(first)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closes_bare_include_image_and_xref() {
        let src = "include::request.adoc\nimage::diagram.png\nxref:other.adoc\nxref:other.adoc#sec\n";
        assert_eq!(
            ensure_macro_attribute_brackets(src).content,
            "include::request.adoc[]\nimage::diagram.png[]\nxref:other.adoc[]\nxref:other.adoc#sec[]\n"
        );
    }

    #[test]
    fn is_idempotent_when_brackets_already_present() {
        let src = "include::request.adoc[]\nimage::x.png[Alt]\nxref:doc.adoc#a[]\n";
        assert_eq!(ensure_macro_attribute_brackets(src).content, src);
    }

    #[test]
    fn skips_listing_literal_and_comment_fences() {
        let src = concat!(
            "include::outer.adoc\n",
            "----\n",
            "include::inside.puml\n",
            "----\n",
            "....\n",
            "xref:literal.adoc\n",
            "....\n",
            "////\n",
            "image::comment.png\n",
            "////\n",
            "include::after.adoc\n",
        );
        assert_eq!(
            ensure_macro_attribute_brackets(src).content,
            concat!(
                "include::outer.adoc[]\n",
                "----\n",
                "include::inside.puml\n",
                "----\n",
                "....\n",
                "xref:literal.adoc\n",
                "....\n",
                "////\n",
                "image::comment.png\n",
                "////\n",
                "include::after.adoc[]\n",
            )
        );
    }

    #[test]
    fn skips_folder_prefixes_and_trailing_hash() {
        let src = "include::shared/\nxref:doc.adoc#\n";
        assert_eq!(ensure_macro_attribute_brackets(src).content, src);
    }

    #[test]
    fn closes_two_macros_on_one_line() {
        let src = "see include::a.adoc and xref:b.adoc#x\n";
        assert_eq!(
            ensure_macro_attribute_brackets(src).content,
            "see include::a.adoc[] and xref:b.adoc#x[]\n"
        );
    }

    #[test]
    fn reports_nothing_when_it_changed_nothing() {
        let src = "= Title\n\ninclude::request.adoc[]\n";
        assert!(ensure_macro_attribute_brackets(src).closed.is_empty());
    }

    #[test]
    fn reports_each_closed_macro_with_its_line() {
        let src = "= Title\n\ninclude::request.adoc\n\nimage::diagram.png\n";
        assert_eq!(
            ensure_macro_attribute_brackets(src).closed,
            vec![
                ClosedMacro { line: 3, text: "include::request.adoc[]".to_string() },
                ClosedMacro { line: 5, text: "image::diagram.png[]".to_string() },
            ]
        );
    }

    #[test]
    fn reports_both_macros_closed_on_one_line() {
        let src = "see include::a.adoc and xref:b.adoc#x\n";
        assert_eq!(
            ensure_macro_attribute_brackets(src).closed,
            vec![
                ClosedMacro { line: 1, text: "include::a.adoc[]".to_string() },
                ClosedMacro { line: 1, text: "xref:b.adoc#x[]".to_string() },
            ]
        );
    }

    #[test]
    fn reports_nothing_for_macros_left_alone_inside_a_fence() {
        // The sample markup in a listing block is untouched, so there is
        // nothing to tell the author about it either.
        let src = "----\ninclude::inside.puml\n----\n";
        let pass = ensure_macro_attribute_brackets(src);
        assert_eq!(pass.content, src);
        assert!(pass.closed.is_empty());
    }
}
