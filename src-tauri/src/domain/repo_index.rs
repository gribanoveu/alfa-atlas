use std::time::SystemTime;

use thiserror::Error;

use super::project_config::ProjectError;
use super::supported_files::extension_of;
use super::workspace_index::WorkspaceIndexError;

/// Bumped whenever indexer behavior changes in a way that would produce
/// different results for the same file content — a new tree-sitter grammar
/// version, a new `SymbolKind`, a new/changed indexer. Nothing persists the
/// index today and nothing reads this yet — it exists so a future on-disk
/// cache or incremental-rebuild check has a cheap staleness signal from day
/// one instead of retrofitting one once indexers have already drifted.
pub const INDEX_VERSION: u32 = 1;

/// Languages this index understands. Deliberately narrow: the repos this
/// app documents are Java backend services with JSON/YAML request and
/// response schemas, described in Markdown/AsciiDoc — not the docflow app's
/// own Rust/TypeScript stack.
///
/// Kotlin was dropped from this list: `tree-sitter-kotlin` is capped at
/// `tree-sitter <0.23`, which conflicts with the newer `tree-sitter`
/// required once `tree-sitter-asciidoc` was prioritized (its compiled
/// grammar needs ABI 15, unsupported by tree-sitter 0.22's runtime) —
/// Cargo's `links = "tree-sitter"` only allows one `tree-sitter` version
/// project-wide. Re-add `Kotlin` if a `tree-sitter-kotlin` release ever
/// raises its own upper bound past 0.23.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Java,
    Json,
    Yaml,
    Markdown,
    AsciiDoc,
}

/// Every `Language` variant — used by tests and by registry construction to
/// confirm every language has a registered indexer.
pub const ALL_LANGUAGES: [Language; 5] = [
    Language::Java,
    Language::Json,
    Language::Yaml,
    Language::Markdown,
    Language::AsciiDoc,
];

/// Extension-based; `None` means the file is not one of the languages this
/// index covers and is skipped entirely by
/// `services::repo_index::RepositoryIndex::build`.
pub fn detect_language(path: &str) -> Option<Language> {
    match extension_of(path).as_str() {
        ".java" => Some(Language::Java),
        ".json" => Some(Language::Json),
        ".yaml" | ".yml" => Some(Language::Yaml),
        ".md" | ".markdown" => Some(Language::Markdown),
        ".adoc" | ".asciidoc" => Some(Language::AsciiDoc),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Class,
    Interface,
    Enum,
    Method,
    Field,
    Section,
}

/// A named, ranged artifact extracted from a file by a `LanguageIndexer`.
/// Carries both line and byte ranges — tree-sitter exposes both on every
/// node already, and throwing them away now only means re-deriving them
/// later for semantic chunking, highlighting, or "go to symbol".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub start_line: u32,
    pub end_line: u32,
    pub start_byte: u32,
    pub end_byte: u32,
}

/// Repo-relative path, `/`-separated — newtype over the key `RepositoryIndex`
/// stores files under, so a bare `String` key isn't ambiguous with any other
/// path-shaped string in the domain.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    pub relative_path: String,
    pub size_bytes: u64,
    pub modified_at: SystemTime,
    pub hash: blake3::Hash,
    pub language: Language,
}

/// One file's structural record. Deliberately does **not** carry file
/// content — a repo of thousands of files would otherwise duplicate the
/// entire working tree in memory before chunks/embeddings/diagnostics even
/// exist. The index describes the project; it doesn't duplicate the
/// filesystem. Callers that need content read it separately (e.g. the
/// `ReadFile` AI tool, or a plain `fs::read_to_string`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    pub metadata: FileMetadata,
    pub symbols: Vec<Symbol>,
}

#[derive(Debug, Error)]
pub enum RepoIndexError {
    #[error("io error: {0}")]
    Io(#[source] std::io::Error),
    #[error("workspace scan failed: {0}")]
    Scan(#[source] WorkspaceIndexError),
    #[error("{0}")]
    Message(String),
}

impl From<WorkspaceIndexError> for RepoIndexError {
    fn from(err: WorkspaceIndexError) -> Self {
        RepoIndexError::Scan(err)
    }
}

impl From<ProjectError> for RepoIndexError {
    fn from(err: ProjectError) -> Self {
        RepoIndexError::Message(err.to_string())
    }
}

/// What a `LanguageIndexer` extracts from one file's content. A struct, not
/// a bare `Vec<Symbol>`, so a future `imports`/`links`/`toc` field is
/// additive — no implementor's signature needs to change to grow this.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LanguageFacts {
    pub symbols: Vec<Symbol>,
}

/// One language's indexing logic. Deliberately infallible — a file that
/// reads fine but is malformed for its language (e.g. broken Java) must
/// still produce a full `IndexedFile` record with real metadata/hash;
/// `index()` returning fewer or zero symbols for bad input is the only
/// failure mode, never an `Err` that would drop the file from the index.
/// Implementors don't report their own `Language` — see
/// `infra::language_indexers::default_indexers` for why that mapping lives
/// in exactly one place instead of two.
pub trait LanguageIndexer: Send + Sync {
    fn index(&self, content: &str) -> LanguageFacts;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_languages() {
        assert_eq!(detect_language("Foo.java"), Some(Language::Java));
        assert_eq!(detect_language("schema.json"), Some(Language::Json));
        assert_eq!(detect_language("config.yaml"), Some(Language::Yaml));
        assert_eq!(detect_language("config.yml"), Some(Language::Yaml));
        assert_eq!(detect_language("README.md"), Some(Language::Markdown));
        assert_eq!(detect_language("README.markdown"), Some(Language::Markdown));
        assert_eq!(detect_language("doc.adoc"), Some(Language::AsciiDoc));
        assert_eq!(detect_language("doc.asciidoc"), Some(Language::AsciiDoc));
    }

    #[test]
    fn rejects_unsupported_extensions() {
        assert_eq!(detect_language("main.rs"), None);
        assert_eq!(detect_language("index.ts"), None);
        assert_eq!(detect_language("notes.txt"), None);
        assert_eq!(detect_language(".gitignore"), None);
        // Kotlin is intentionally unsupported — see the `Language` doc comment.
        assert_eq!(detect_language("Foo.kt"), None);
        assert_eq!(detect_language("Foo.kts"), None);
    }
}
