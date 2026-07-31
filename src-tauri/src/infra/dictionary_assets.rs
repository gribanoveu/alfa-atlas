//! Built-in Hunspell dictionary data, embedded at compile time.
//!
//! Embedding (rather than shipping as Tauri bundle resources) sidesteps the
//! resource-path differences between `cargo tauri dev` and a bundled build —
//! the data is simply part of the binary, in dev and in production alike.
//! Source: LibreOffice/dictionaries (MPL/GPL/LGPL tri-licensed) — see the
//! LICENSE/README files alongside each pair under `src-tauri/dictionaries/`.

const EN_US_AFF: &str = include_str!("../../dictionaries/en_US/en_US.aff");
const EN_US_DIC: &str = include_str!("../../dictionaries/en_US/en_US.dic");
const RU_RU_AFF: &str = include_str!("../../dictionaries/ru_RU/ru_RU.aff");
const RU_RU_DIC: &str = include_str!("../../dictionaries/ru_RU/ru_RU.dic");

/// Team-maintained flat word list (acronyms/jargon), edited directly in the
/// repo — unlike the per-user personal dictionary in `~/.atlas`, this ships
/// with the app and is shared by everyone.
const INTERNAL_WORDLIST: &str = include_str!("../../dictionaries/internal/internal.txt");

/// Returns the `(aff, dic)` source pair for a built-in Hunspell dictionary
/// id, or `None` if `id` isn't a known Hunspell-format built-in (e.g. it's
/// the custom dictionary, or the flat-word-list `internal` dictionary).
pub fn builtin_source(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "en_US" => Some((EN_US_AFF, EN_US_DIC)),
        "ru_RU" => Some((RU_RU_AFF, RU_RU_DIC)),
        _ => None,
    }
}

/// Raw contents of the built-in technical/internal word list (`#`-prefixed
/// lines are comments, one word per line otherwise).
pub fn internal_wordlist() -> &'static str {
    INTERNAL_WORDLIST
}
