//! Deliberately extracts no symbols — same rationale as `JsonIndexer` (see
//! that module's doc comment): a line-based `key:` regex scan produces
//! false positives on values containing a colon, and `serde_yaml::Value`
//! carries no position information. The file is still indexed (metadata +
//! hash + language); only `.symbols` is empty.

use crate::domain::repo_index::{LanguageFacts, LanguageIndexer};

pub struct YamlIndexer;

impl LanguageIndexer for YamlIndexer {
    fn index(&self, _content: &str) -> LanguageFacts {
        LanguageFacts::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_returns_no_symbols() {
        let facts = YamlIndexer.index("description: \"field:value\"\n");
        assert!(facts.symbols.is_empty());
    }
}
