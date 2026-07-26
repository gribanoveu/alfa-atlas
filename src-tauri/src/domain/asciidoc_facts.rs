//! IPC contract types for the async AsciiDoc parse flow.
//!
//! The Rust backend no longer parses AsciiDoc content itself in production.
//! Instead, when a `.adoc` / `.asciidoc` file is indexed, `WorkspaceIndex`
//! emits an `asciidoc:parse-requested` event carrying the file content to
//! the frontend. The frontend runs `asciidoctor.js`, walks the AST, and
//! calls the `submit_asciidoc_facts` Tauri command with the extracted facts.
//!
//! The `document` / `source_document` fields on the domain entities
//! (`Anchor`, `Include`, `Reference`, `Attribute`, `Image`) are NOT
//! transmitted by the frontend — Rust fills them in from the `document_id`
//! argument of the `submit_asciidoc_facts` command. This keeps the payload
//! small and prevents frontend/backend drift on the document identity.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::workspace_index::DocumentId;

/// Payload of the `asciidoc:parse-requested` event (Rust -> Frontend).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AsciiDocParseRequested {
    pub document_id: DocumentId,
    /// Monotonic per-document version. The frontend echoes this back in
    /// `submit_asciidoc_facts`; Rust discards responses whose version does
    /// not match the current `doc_versions[document_id]`.
    pub version: u64,
    pub content: String,
    /// Relative path of the document, included for potential future use
    /// (e.g., resolving relative `image::` paths in the frontend). Not
    /// currently consumed by `extractFacts`.
    pub relative_path: PathBuf,
}

/// Payload of the `submit_asciidoc_facts` command (Frontend -> Rust).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AsciiDocFacts {
    pub anchors: Vec<AnchorFact>,
    pub includes: Vec<IncludeFact>,
    pub references: Vec<ReferenceFact>,
    pub attributes: Vec<AttributeFact>,
    pub images: Vec<ImageFact>,
    pub parse_errors: Vec<ParseErrorFact>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorFact {
    pub id: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncludeFact {
    pub path: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceFact {
    pub target_document: String,
    pub anchor: Option<String>,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeFact {
    pub name: String,
    pub value: String,
    pub line: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageFact {
    pub path: String,
    pub line: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseErrorFact {
    pub message: String,
    pub line: Option<u32>,
    /// Severity label as reported by `asciidoctor.js`'s `LogMessage.getSeverity()`
    /// (e.g. `"ERROR"`, `"WARN"`, `"INFO"`). Defaults to `"error"` when absent
    /// so pre-existing callers keep their semantics. Only `"ERROR"` is mapped
    /// to `Severity::Error`; everything else degrades to `Severity::Warning`
    /// — table-layout quirks like "dropping cells from incomplete row" are
    /// reported by asciidoctor at WARN level and must not flip the index
    /// status to "failed".
    #[serde(default = "default_severity")]
    pub severity: String,
}

fn default_severity() -> String {
    "error".to_string()
}
