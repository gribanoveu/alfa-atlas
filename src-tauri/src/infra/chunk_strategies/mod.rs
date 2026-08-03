pub mod asciidoc;
pub mod java;
pub mod markdown;
pub mod whole_file;

use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::chunk_index::ChunkStrategy;
use crate::domain::repo_index::Language;

/// Explicit, one-time registration of which `ChunkStrategy` handles each
/// `Language` — mirrors `infra::language_indexers::default_indexers`: no
/// `supports()` self-reporting, one literal `HashMap`, one source of truth
/// for the mapping.
pub fn default_chunk_strategies() -> HashMap<Language, Arc<dyn ChunkStrategy>> {
    let mut map: HashMap<Language, Arc<dyn ChunkStrategy>> = HashMap::new();
    map.insert(Language::Java, Arc::new(java::JavaChunkStrategy));
    map.insert(Language::Json, Arc::new(whole_file::WholeFileChunkStrategy));
    map.insert(Language::Yaml, Arc::new(whole_file::WholeFileChunkStrategy));
    map.insert(Language::Markdown, Arc::new(markdown::MarkdownChunkStrategy));
    map.insert(Language::AsciiDoc, Arc::new(asciidoc::AsciiDocChunkStrategy));
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repo_index::ALL_LANGUAGES;

    #[test]
    fn registers_every_known_language() {
        let strategies = default_chunk_strategies();
        for language in ALL_LANGUAGES {
            assert!(
                strategies.contains_key(&language),
                "no chunk strategy registered for {language:?}"
            );
        }
    }
}
