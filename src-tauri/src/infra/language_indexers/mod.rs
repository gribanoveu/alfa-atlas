pub mod asciidoc;
pub mod java;
pub mod json;
pub mod markdown;
pub mod yaml;

use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::repo_index::{Language, LanguageIndexer};

/// Explicit, one-time registration of which `LanguageIndexer` handles each
/// `Language` — the only place this mapping is described. Indexers don't
/// self-report support via a `supports()` method: that would be a second,
/// independently-editable copy of the exact mapping this function already
/// encodes as a literal `HashMap`.
pub fn default_indexers() -> HashMap<Language, Arc<dyn LanguageIndexer>> {
    let mut map: HashMap<Language, Arc<dyn LanguageIndexer>> = HashMap::new();
    map.insert(Language::Java, Arc::new(java::JavaIndexer));
    map.insert(Language::Json, Arc::new(json::JsonIndexer));
    map.insert(Language::Yaml, Arc::new(yaml::YamlIndexer));
    map.insert(Language::Markdown, Arc::new(markdown::MarkdownIndexer));
    map.insert(Language::AsciiDoc, Arc::new(asciidoc::AsciiDocIndexer));
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repo_index::ALL_LANGUAGES;

    #[test]
    fn registers_every_known_language() {
        let indexers = default_indexers();
        for language in ALL_LANGUAGES {
            assert!(
                indexers.contains_key(&language),
                "no indexer registered for {language:?}"
            );
        }
    }
}
