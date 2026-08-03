//! Deliberately extracts no symbols.
//!
//! A regex key-scan (`"key":`) produces false positives on quoted values
//! that merely contain a colon (`{"query": "field:value"}`), and
//! `serde_json::Value` carries no position information to do this properly
//! without one. An empty, honest symbol list beats an inaccurate one — the
//! file is still indexed (metadata + hash + language) by
//! `services::repo_index::RepositoryIndex::build`; only `.symbols` is empty.
//! Real position-aware key extraction is future work once a consumer
//! actually needs it.

use crate::domain::repo_index::{LanguageFacts, LanguageIndexer};

pub struct JsonIndexer;

impl LanguageIndexer for JsonIndexer {
    fn index(&self, _content: &str) -> LanguageFacts {
        LanguageFacts::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_returns_no_symbols() {
        let facts = JsonIndexer.index(r#"{"a": {"b": "c:d"}}"#);
        assert!(facts.symbols.is_empty());
    }
}
