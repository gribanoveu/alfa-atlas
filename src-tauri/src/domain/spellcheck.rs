//! Domain types for the spellchecker.
//!
//! Rule/engine logic (tokenization, dictionary lookups) lives in
//! `services/spellcheck.rs` — these types are the serializable data shapes
//! shared between the engine, the persisted config, and the frontend.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Static metadata for one built-in dictionary. The word data itself is
/// embedded separately in `infra/dictionary_assets.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryDef {
    pub id: &'static str,
    pub title: &'static str,
}

pub const BUILTIN_DICTIONARIES: &[DictionaryDef] = &[
    DictionaryDef {
        id: "ru_RU",
        title: "Русский",
    },
    DictionaryDef {
        id: "en_US",
        title: "Английский",
    },
    DictionaryDef {
        id: "internal",
        title: "Технический (встроенный)",
    },
];

/// Persisted spellcheck settings: a master on/off switch plus per-dictionary
/// enable/disable overrides, keyed by `DictionaryDef::id`. A dictionary
/// absent from the map defaults to enabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpellcheckConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub dictionaries: HashMap<String, bool>,
    /// Skip words shaped like `getUserInfo`/`isEnabled` — identifiers
    /// mentioned inline in prose without code formatting. On by default:
    /// these are essentially never genuine spelling mistakes.
    #[serde(default = "default_true")]
    pub skip_camel_case: bool,
}

fn default_true() -> bool {
    true
}

impl Default for SpellcheckConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dictionaries: HashMap::new(),
            skip_camel_case: true,
        }
    }
}

impl SpellcheckConfig {
    pub fn is_dictionary_enabled(&self, id: &str) -> bool {
        self.dictionaries.get(id).copied().unwrap_or(true)
    }
}

/// The document "shape" driving how spellcheckable text is extracted, since
/// the frontend has no richer per-document language/format metadata than
/// this. Mirrors `spellcheckKindFor()` on the TypeScript side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocKind {
    Markdown,
    Asciidoc,
    Plain,
}

/// One misspelled word found in a document. Deliberately carries no
/// suggestions — those are expensive to compute and fetched on demand via a
/// separate command only for the word under the cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpellIssue {
    pub line: u32,
    pub column: u32,
    pub length: u32,
    pub word: String,
}

#[derive(Debug, Error)]
pub enum SpellcheckError {
    #[error("home directory is unavailable")]
    HomeDirUnavailable,
    #[error("failed to create dictionary directory: {0}")]
    CreateDir(#[source] std::io::Error),
    #[error("failed to read custom dictionary: {0}")]
    Read(#[source] std::io::Error),
    #[error("failed to write custom dictionary: {0}")]
    Write(#[source] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_falls_back_to_enabled() {
        let config = SpellcheckConfig::default();
        assert!(config.is_dictionary_enabled("ru_RU"));

        let mut config = SpellcheckConfig::default();
        config.dictionaries.insert("ru_RU".to_string(), false);
        assert!(!config.is_dictionary_enabled("ru_RU"));
        assert!(config.is_dictionary_enabled("en_US"));
    }

    #[test]
    fn deserializes_legacy_json_without_fields() {
        let config: SpellcheckConfig = serde_json::from_str("{}").unwrap();
        assert!(config.enabled);
        assert!(config.dictionaries.is_empty());
        assert!(config.skip_camel_case);
    }
}
