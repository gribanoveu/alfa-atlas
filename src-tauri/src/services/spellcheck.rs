//! Spellcheck engine: lazily-built Hunspell dictionaries + the in-memory
//! personal word list, kept as Tauri-managed state so dictionaries are
//! parsed once per app session instead of once per keystroke.

use std::collections::{BTreeSet, HashSet};
use std::sync::{Arc, LazyLock};

use dashmap::{DashMap, DashSet};
use pulldown_cmark::{Event, Options, Parser as MdParser};
use regex::Regex;

use crate::domain::spellcheck::{
    BUILTIN_DICTIONARIES, DocKind, SpellIssue, SpellcheckConfig, SpellcheckError,
};
use crate::infra::{custom_dictionary_store, dictionary_assets};

const MAX_SUGGESTIONS: usize = 5;
const INTERNAL_DICTIONARY_ID: &str = "internal";

/// A built-in dictionary is either a real Hunspell dictionary (morphology,
/// suggestions) or a flat, team-maintained word list (the `internal`
/// dictionary) — acronyms and jargon don't need affix rules, and a flat set
/// is much simpler to hand-edit than a `.aff`/`.dic` pair.
enum DictionarySource {
    Hunspell(spellbook::Dictionary),
    WordList(HashSet<String>),
}

impl DictionarySource {
    fn check(&self, word: &str) -> bool {
        match self {
            Self::Hunspell(dict) => dict.check(word),
            Self::WordList(words) => {
                words.contains(word) || words.contains(&word.to_lowercase())
            }
        }
    }

    fn suggest(&self, word: &str, out: &mut Vec<String>) {
        if let Self::Hunspell(dict) = self {
            dict.suggest(word, out);
        }
        // Flat word lists have no morphology to derive suggestions from.
    }
}

/// Unicode-aware word token: a leading letter followed by letters, combining
/// marks, apostrophes or hyphens — covers Cyrillic and Latin text alike.
static WORD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{L}][\p{L}\p{Mn}'’-]*").expect("valid regex"));

// Rust's `regex` crate has no backreferences, so this doesn't require the
// closing delimiter to match the opening one's exact dash/dot count — in
// practice AsciiDoc listing/fence blocks always use the same delimiter, so
// matching "opening fence line ... next fence line of the same kind" is
// equivalent for real documents.
static ADOC_FENCE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ms)^-{4,}[ \t]*\n.*?^-{4,}[ \t]*$|^\.{4,}[ \t]*\n.*?^\.{4,}[ \t]*$")
        .expect("valid regex")
});
static ADOC_INLINE_CODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`[^`\n]*`|\+[^+\n]*\+").expect("valid regex"));
// Masks only the macro name + target (`image::foo.png`, `xref:doc.adoc`,
// `link:https://x`), not the trailing `[...]` — the bracket text is often
// real prose (`xref:doc.adoc[See the installation guide]`) and should still
// be spellchecked.
static ADOC_MACRO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Za-z][A-Za-z0-9]*:{1,2}[^\[\s]*").expect("valid regex"));
static ADOC_ATTR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{[A-Za-z0-9_-]+\}").expect("valid regex"));
// Attribute *definition* lines (`:toc: left`, `:sectnums:`) — only the
// `:name:` marker is syntax; any value after it is real document text.
static ADOC_ATTR_DEF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^:[A-Za-z][\w-]*:").expect("valid regex"));
// Whole-line block attribute lists (`[source,json]`, `[cols="1,1"]`,
// `[#anchor]`, `[[anchor-id]]`, `[.role]`) — never prose, so mask the line.
static ADOC_BLOCK_ATTR_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*\[+[^\]\n]*\]+[ \t]*$").expect("valid regex"));
static URL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w+://\S+").expect("valid regex"));

pub struct SpellcheckEngine {
    dictionaries: DashMap<&'static str, Arc<DictionarySource>>,
    custom_words: DashSet<String>,
}

impl Default for SpellcheckEngine {
    fn default() -> Self {
        Self {
            dictionaries: DashMap::new(),
            custom_words: DashSet::new(),
        }
    }
}

impl SpellcheckEngine {
    /// Loads the persisted personal dictionary from `~/.atlas/dictionaries/custom.txt`.
    /// Use this for the real app; tests should stick to `default()`/`new()` so
    /// they never read or write the user's actual `~/.atlas` state.
    pub fn load() -> Self {
        let engine = Self::default();
        for word in custom_dictionary_store::load_custom_words().unwrap_or_default() {
            engine.custom_words.insert(word);
        }
        engine
    }

    fn dictionary(&self, id: &'static str) -> Option<Arc<DictionarySource>> {
        if let Some(existing) = self.dictionaries.get(id) {
            return Some(existing.clone());
        }
        let source = if id == INTERNAL_DICTIONARY_ID {
            DictionarySource::WordList(
                dictionary_assets::internal_wordlist()
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .map(str::to_string)
                    .collect(),
            )
        } else {
            let (aff, dic) = dictionary_assets::builtin_source(id)?;
            DictionarySource::Hunspell(spellbook::Dictionary::new(aff, dic).ok()?)
        };
        let source = Arc::new(source);
        self.dictionaries.insert(id, source.clone());
        Some(source)
    }

    fn enabled_dictionaries(&self, config: &SpellcheckConfig) -> Vec<Arc<DictionarySource>> {
        BUILTIN_DICTIONARIES
            .iter()
            .filter(|def| config.is_dictionary_enabled(def.id))
            .filter_map(|def| self.dictionary(def.id))
            .collect()
    }

    /// Cheap pass: tokenizes `text` per `kind` and returns every word that
    /// fails every enabled dictionary (custom + built-in, union semantics).
    /// Deliberately doesn't compute suggestions — see `suggest`.
    pub fn check_text(&self, text: &str, kind: DocKind, config: &SpellcheckConfig) -> Vec<SpellIssue> {
        if !config.enabled {
            return Vec::new();
        }
        let dicts = self.enabled_dictionaries(config);
        if dicts.is_empty() {
            return Vec::new();
        }

        let tokens = match kind {
            DocKind::Markdown => tokenize_markdown(text),
            DocKind::Asciidoc => tokenize_words(&mask_asciidoc(text)),
            DocKind::Plain => tokenize_words(text),
        };

        let line_starts = build_line_starts(text);

        tokens
            .into_iter()
            .flat_map(|(offset, word)| self.decompose(offset, word, &dicts, config))
            .filter(|(_, word)| !self.is_known(word, &dicts))
            .map(|(offset, word)| {
                let (line, column) = line_col_for(offset, &line_starts, text);
                SpellIssue {
                    line,
                    column,
                    length: word.chars().count() as u32,
                    word,
                }
            })
            .collect()
    }

    fn is_known(&self, word: &str, dicts: &[Arc<DictionarySource>]) -> bool {
        self.custom_words.contains(word)
            || self.custom_words.contains(&word.to_lowercase())
            || dicts.iter().any(|d| d.check(word))
    }

    /// Breaks a compound token into checkable pieces, but only when the
    /// whole thing isn't already a recognized word — this keeps curated
    /// whole-phrase dictionary entries (`OpenAPI`, `mks-users-api`) matching
    /// exactly as before, while still catching compounds nobody curated
    /// (`annual-tax-report-api`, `DownloadNotificationService`) by falling
    /// back to word-by-word checks. Recurses so a hyphenated part that is
    /// itself PascalCase/camelCase gets split further.
    fn decompose(
        &self,
        offset: usize,
        word: String,
        dicts: &[Arc<DictionarySource>],
        config: &SpellcheckConfig,
    ) -> Vec<(usize, String)> {
        if self.is_known(&word, dicts) {
            return vec![(offset, word)];
        }

        // Checking `word.contains('-')` rather than `hyphen_parts.len() > 1`
        // matters for diagram/arrow syntax like `PGW-->CTRL`, which the
        // tokenizer regex greedily captures as `PGW--`: after dropping the
        // empty pieces around consecutive/trailing hyphens only one part
        // (`PGW`) is left, but that stripped part must still replace the
        // untrimmed original rather than falling through to be checked as
        // literal `PGW--`.
        if word.contains('-') {
            let hyphen_parts = split_hyphenated(offset, &word);
            return hyphen_parts
                .into_iter()
                .flat_map(|(o, w)| self.decompose(o, w, dicts, config))
                .collect();
        }

        // No hyphen left to split on. Split at case boundaries too, unless
        // this is a lowerCamelCase identifier (`getUserInfo`) and the user
        // has disabled `skip_camel_case` — in that case they want it judged
        // as one literal blob, same as before this compound-splitting logic
        // existed.
        if !is_camel_case(&word) || config.skip_camel_case {
            return split_case_boundaries(offset, &word);
        }
        vec![(offset, word)]
    }

    /// Suggestions for one word, computed on demand (not during `check_text`)
    /// since `Dictionary::suggest` is significantly more expensive than
    /// `check` — doing this eagerly for every misspelled word on every
    /// debounced keystroke would not scale on longer documents.
    pub fn suggest(&self, word: &str, config: &SpellcheckConfig) -> Vec<String> {
        let mut merged = Vec::new();
        for dict in self.enabled_dictionaries(config) {
            let mut suggestions = Vec::new();
            dict.suggest(word, &mut suggestions);
            for s in suggestions {
                if !merged.contains(&s) {
                    merged.push(s);
                }
                if merged.len() >= MAX_SUGGESTIONS {
                    return merged;
                }
            }
        }
        merged
    }

    pub fn custom_words(&self) -> Vec<String> {
        let mut words: Vec<String> = self.custom_words.iter().map(|w| w.key().clone()).collect();
        words.sort();
        words
    }

    pub fn add_custom_word(&self, word: String) -> Result<(), SpellcheckError> {
        let trimmed = word.trim().to_string();
        if trimmed.is_empty() {
            return Ok(());
        }
        self.custom_words.insert(trimmed);
        self.persist_custom_words()
    }

    pub fn remove_custom_word(&self, word: &str) -> Result<(), SpellcheckError> {
        self.custom_words.remove(word);
        self.persist_custom_words()
    }

    fn persist_custom_words(&self) -> Result<(), SpellcheckError> {
        let set: BTreeSet<String> = self.custom_words.iter().map(|w| w.key().clone()).collect();
        custom_dictionary_store::save_custom_words(&set)
    }
}

/// True for identifier-shaped words like `getUserInfo`/`isEnabled`: starts
/// with a lowercase letter and has an uppercase letter somewhere after it.
/// Deliberately narrower than PascalCase/ALLCAPS, which real words can
/// collide with (e.g. a sentence-initial capitalized word).
fn is_camel_case(word: &str) -> bool {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => chars.any(|c| c.is_uppercase()),
        _ => false,
    }
}

/// Splits a compound word on hyphens so names like `annual-tax-report-api`
/// can be checked word-by-word instead of as one never-in-any-dictionary
/// blob. Byte offsets are recomputed per part so issue positions still
/// point at just the misspelled component. Called from `decompose` only
/// after the whole word has failed a dictionary lookup.
fn split_hyphenated(offset: usize, word: &str) -> Vec<(usize, String)> {
    if !word.contains('-') {
        return vec![(offset, word.to_string())];
    }
    let mut out = Vec::new();
    let mut pos = 0;
    for part in word.split('-') {
        if !part.is_empty() {
            out.push((offset + pos, part.to_string()));
        }
        pos += part.len() + 1; // +1 for the hyphen byte just consumed
    }
    out
}

/// Splits a word at case-transition boundaries: lowercase-to-uppercase
/// (`Download|Notification|Service`) and the trailing edge of an acronym run
/// (`HTTP|Server`). Words without an internal transition (plain lowercase,
/// a single leading capital, or a standalone ALLCAPS acronym) come back
/// unchanged as one part. Called from `decompose` only after the whole word
/// has failed a dictionary lookup.
fn split_case_boundaries(offset: usize, word: &str) -> Vec<(usize, String)> {
    let indices: Vec<(usize, char)> = word.char_indices().collect();
    if indices.len() < 2 {
        return vec![(offset, word.to_string())];
    }
    let mut boundaries = vec![0usize];
    for i in 1..indices.len() {
        let prev = indices[i - 1].1;
        let cur = indices[i].1;
        let lower_to_upper = prev.is_lowercase() && cur.is_uppercase();
        let acronym_boundary = prev.is_uppercase()
            && cur.is_uppercase()
            && i + 1 < indices.len()
            && indices[i + 1].1.is_lowercase();
        if lower_to_upper || acronym_boundary {
            boundaries.push(i);
        }
    }
    boundaries.push(indices.len());
    boundaries
        .windows(2)
        .map(|w| {
            let byte_start = indices[w[0]].0;
            let byte_end = indices.get(w[1]).map_or(word.len(), |(b, _)| *b);
            (offset + byte_start, word[byte_start..byte_end].to_string())
        })
        .collect()
}

fn tokenize_words(text: &str) -> Vec<(usize, String)> {
    WORD_RE
        .find_iter(text)
        .map(|m| (m.start(), m.as_str().to_string()))
        .collect()
}

/// Walks Markdown structure via `pulldown-cmark` and only extracts words
/// from `Event::Text` — code spans, fenced blocks, and link destinations are
/// separate event kinds and never scanned.
fn tokenize_markdown(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (event, range) in MdParser::new_ext(text, Options::empty()).into_offset_iter() {
        if matches!(event, Event::Text(_)) {
            let slice = &text[range.clone()];
            for m in WORD_RE.find_iter(slice) {
                out.push((range.start + m.start(), m.as_str().to_string()));
            }
        }
    }
    out
}

/// AsciiDoc has no Rust-side parser (asciidoctor.js is frontend-preview-only)
/// so this replaces non-prose regions — fenced/listing blocks, inline code,
/// block attribute lines/anchors, attribute definitions, macro names/targets,
/// attribute refs, bare URLs — with spaces (never removing bytes) so every
/// remaining byte offset still lines up with the original document for
/// correct line/column reporting.
fn mask_asciidoc(text: &str) -> String {
    let mut bytes = text.as_bytes().to_vec();
    for re in [
        &*ADOC_FENCE_RE,
        &*ADOC_INLINE_CODE_RE,
        &*ADOC_BLOCK_ATTR_LINE_RE,
        &*ADOC_ATTR_DEF_RE,
        &*ADOC_MACRO_RE,
        &*ADOC_ATTR_RE,
        &*URL_RE,
    ] {
        for m in re.find_iter(text) {
            for b in &mut bytes[m.start()..m.end()] {
                if *b != b'\n' {
                    *b = b' ';
                }
            }
        }
    }
    // Every replaced byte is single-byte ASCII (space/newline) and full
    // matched spans are replaced wholesale, so the buffer stays valid UTF-8.
    String::from_utf8(bytes).unwrap_or_else(|_| text.to_string())
}

fn build_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

fn line_col_for(byte_offset: usize, line_starts: &[usize], text: &str) -> (u32, u32) {
    let line_idx = match line_starts.binary_search(&byte_offset) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };
    let line_start = line_starts[line_idx];
    let column = text[line_start..byte_offset].chars().count() as u32 + 1;
    (line_idx as u32 + 1, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(enabled_ids: &[&str]) -> SpellcheckConfig {
        let mut config = SpellcheckConfig {
            enabled: true,
            dictionaries: BUILTIN_DICTIONARIES
                .iter()
                .map(|d| (d.id.to_string(), false))
                .collect(),
            skip_camel_case: true,
            check_txt: false,
        };
        for id in enabled_ids {
            config.dictionaries.insert((*id).to_string(), true);
        }
        config
    }

    #[test]
    fn flags_unknown_word_in_plain_text() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US"]);
        let issues = engine.check_text("hello wrold", DocKind::Plain, &config);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].word, "wrold");
        assert_eq!(issues[0].line, 1);
        assert_eq!(issues[0].column, 7);
    }

    #[test]
    fn skips_camel_case_words_by_default() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US"]);
        assert!(config.skip_camel_case);
        let issues = engine.check_text("call getUserInfo now", DocKind::Plain, &config);
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    #[test]
    fn checks_camel_case_words_when_disabled() {
        let engine = SpellcheckEngine::default();
        let mut config = config_with(&["en_US"]);
        config.skip_camel_case = false;
        let issues = engine.check_text("call getUserInfo now", DocKind::Plain, &config);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].word, "getUserInfo");
    }

    #[test]
    fn plain_uppercase_and_lowercase_words_are_unaffected_by_camel_case_skip() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US"]);
        // Neither ALLCAPS nor a sentence-initial capitalized real word should
        // be mistaken for camelCase and skipped.
        assert!(engine
            .check_text("HELLO Hello hello", DocKind::Plain, &config)
            .is_empty());
    }

    #[test]
    fn union_across_enabled_dictionaries() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US", "ru_RU"]);
        // "hello" is valid English, "привет" is valid Russian — a union
        // check must accept both without either dictionary alone covering
        // them all.
        let issues = engine.check_text("hello привет", DocKind::Plain, &config);
        assert!(issues.is_empty());
    }

    #[test]
    fn custom_word_suppresses_flag() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US"]);
        assert_eq!(
            engine.check_text("wrold", DocKind::Plain, &config).len(),
            1
        );
        // Insert directly into the in-memory set — `add_custom_word` also
        // persists to the user's real `~/.atlas/dictionaries/custom.txt`,
        // which a unit test must never touch.
        engine.custom_words.insert("wrold".to_string());
        assert!(engine.check_text("wrold", DocKind::Plain, &config).is_empty());
    }

    #[test]
    fn disabled_globally_returns_no_issues() {
        let engine = SpellcheckEngine::default();
        let mut config = config_with(&["en_US"]);
        config.enabled = false;
        assert!(engine
            .check_text("wrold", DocKind::Plain, &config)
            .is_empty());
    }

    #[test]
    fn markdown_skips_code_spans() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US"]);
        let issues = engine.check_text("Use `wrold_fn()` here", DocKind::Markdown, &config);
        assert!(issues.is_empty());
    }

    #[test]
    fn asciidoc_skips_listing_blocks_and_macros() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US"]);
        let text = "----\nwrold\n----\n\nimage::wrold.png[]\n\n{wrold}\n";
        let issues = engine.check_text(text, DocKind::Asciidoc, &config);
        assert!(issues.is_empty());
    }

    #[test]
    fn asciidoc_skips_block_attribute_lines_anchors_and_attribute_defs() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US"]);
        let text = "[source,json]\n----\nwrold\n----\n\n[[wrold-anchor]]\n\n:wrold: 1\n\n[.wrold-role]\nSome text\n";
        let issues = engine.check_text(text, DocKind::Asciidoc, &config);
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    #[test]
    fn asciidoc_still_checks_prose_inside_macro_brackets() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US", "ru_RU"]);
        // The macro name/target (`xref:doc.adoc`) is syntax and must be
        // masked, but the bracket text is real, user-facing prose and must
        // still be spellchecked — only "wrold" here is a genuine typo.
        let issues = engine.check_text(
            "xref:doc.adoc[Установка wrold]\n",
            DocKind::Asciidoc,
            &config,
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].word, "wrold");
    }

    #[test]
    fn internal_dictionary_recognizes_technical_terms() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["internal"]);
        assert!(engine
            .check_text("API ФНС OpenAPI", DocKind::Plain, &config)
            .is_empty());
        // Sanity check: the internal word list isn't a catch-all — a random
        // non-word should still be flagged.
        assert_eq!(
            engine.check_text("asdkjasjd", DocKind::Plain, &config).len(),
            1
        );
    }

    #[test]
    fn hyphenated_compound_is_checked_word_by_word() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US", "internal"]);
        // "annual", "tax" and "report" are real English words and "api" is
        // in the internal dictionary — none should be flagged individually,
        // even though the whole hyphenated blob is in no dictionary.
        assert!(engine
            .check_text("annual-tax-report-api", DocKind::Plain, &config)
            .is_empty());

        // A genuine typo inside one hyphen-separated part must still be
        // caught, at the position of just that part.
        let issues = engine.check_text("annual-taxx-report", DocKind::Plain, &config);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].word, "taxx");
        assert_eq!(issues[0].column, 8);
    }

    #[test]
    fn pascal_case_compound_is_checked_word_by_word() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US"]);
        // "Download", "Notification" and "Service" are all real words —
        // none should be flagged, even though the whole PascalCase blob is
        // in no dictionary.
        assert!(engine
            .check_text("DownloadNotificationService", DocKind::Plain, &config)
            .is_empty());

        // A typo inside one of the case-delimited parts must still be
        // caught, at the position of just that part.
        let issues = engine.check_text("DownloadNotificatoinService", DocKind::Plain, &config);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].word, "Notificatoin");
        assert_eq!(issues[0].column, 9);
    }

    #[test]
    fn lower_camel_case_is_checked_word_by_word_by_default() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US"]);
        // "get", "User" and "Info" are all real words — the identifier as a
        // whole is in no dictionary, but by default it's split at case
        // boundaries and checked part-by-part rather than skipped outright.
        assert!(engine
            .check_text("call getUserInfo now", DocKind::Plain, &config)
            .is_empty());

        // A typo inside one of the parts is still caught, at the position
        // of just that part.
        let issues = engine.check_text("call getUserInfoo now", DocKind::Plain, &config);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].word, "Infoo");

        // With the split disabled, the identifier is judged literally as
        // one blob instead — which almost never matches a dictionary word.
        let mut config = config;
        config.skip_camel_case = false;
        let issues = engine.check_text("call getUserInfo now", DocKind::Plain, &config);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].word, "getUserInfo");
    }

    #[test]
    fn curated_hyphenated_dictionary_entry_is_matched_whole_before_splitting() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["internal"]);
        // "mks-users-api" is curated as a literal whole-phrase entry in the
        // internal dictionary — it must match directly rather than being
        // split into "mks"/"users"/"api" (of which "mks" alone isn't a
        // recognized word).
        assert!(engine
            .check_text("mks-users-api", DocKind::Plain, &config)
            .is_empty());

        // Likewise "OpenAPI" is a curated whole term and shouldn't be split
        // into "Open" + "API".
        assert!(engine
            .check_text("OpenAPI", DocKind::Plain, &config)
            .is_empty());
    }

    #[test]
    fn arrow_syntax_trailing_hyphens_are_stripped_before_checking() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US"]);
        engine.custom_words.insert("pgw".to_string());
        // Diagram/sequence syntax like `PGW-->CTRL` or `PGW->CTRL` tokenizes
        // as `PGW--`/`PGW-` (the tokenizer regex greedily eats trailing
        // hyphens). Both must resolve to the bare word `PGW` rather than
        // being flagged as an unrecognized `PGW--`/`PGW-` blob.
        assert!(engine
            .check_text("PGW-->call", DocKind::Plain, &config)
            .is_empty());
        assert!(engine
            .check_text("PGW->call", DocKind::Plain, &config)
            .is_empty());
    }

    #[test]
    fn suggest_returns_close_matches() {
        let engine = SpellcheckEngine::default();
        let config = config_with(&["en_US"]);
        let suggestions = engine.suggest("wrold", &config);
        assert!(suggestions.iter().any(|s| s == "world"));
    }
}
