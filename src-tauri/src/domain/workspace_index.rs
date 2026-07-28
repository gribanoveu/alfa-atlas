use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Relative path normalized as a key. Equal relative paths share the same `DocumentId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentId(pub String);

impl DocumentId {
    pub fn new(relative_path: impl Into<String>) -> Self {
        Self(relative_path.into())
    }

    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for DocumentId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for DocumentId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DocumentType {
    AsciiDoc,
    Markdown,
    Json,
    Yaml,
    Text,
    PlantUml,
    Mermaid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: DocumentId,
    pub absolute_path: String,
    pub relative_path: String,
    pub file_name: String,
    pub doc_type: DocumentType,
    /// Unix timestamp (seconds).
    pub modified_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    pub id: String,
    pub document: DocumentId,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Include {
    pub path: String,
    pub source_document: DocumentId,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    pub target_document: String,
    pub anchor: Option<String>,
    pub source_document: DocumentId,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribute {
    pub name: String,
    pub value: String,
    pub document: DocumentId,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub path: String,
    pub document: DocumentId,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticKind {
    MissingInclude,
    MissingXrefDocument,
    MissingXrefAnchor,
    MissingImage,
    DuplicateAnchor,
    CircularInclude,
    ParseError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub message: String,
    pub document: DocumentId,
    pub line: u32,
    pub column: u32,
    pub severity: Severity,
}

/// Aggregated result of parsing one document. Parsers return this regardless of
/// source format; fields default to empty for formats that don't expose them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedDocument {
    pub anchors: Vec<Anchor>,
    pub includes: Vec<Include>,
    pub references: Vec<Reference>,
    pub attributes: Vec<Attribute>,
    pub images: Vec<Image>,
    /// Parse-time warnings/errors (syntax issues), keyed to lines in the source.
    pub diagnostics: Vec<Diagnostic>,
}

/// Lightweight statistics published alongside `IndexBuildingFinished`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    pub documents: u32,
    pub anchors: u32,
    pub includes: u32,
    pub references: u32,
    pub attributes: u32,
    pub images: u32,
    pub warnings: u32,
    pub errors: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "payload")]
pub enum IndexEvent {
    IndexBuildingStarted,
    IndexBuildingProgress { done: u32, total: u32, current: String },
    IndexBuildingFinished { stats: IndexStats },
    IndexUpdated { document: String },
    DiagnosticsUpdated { document: String },
}

#[derive(Debug, Error)]
pub enum WorkspaceIndexError {
    #[error("io error: {0}")]
    Io(#[source] std::io::Error),
    #[error("workspace index is not open; call build_index first")]
    NotOpen,
    #[error("path escapes workspace root: {0}")]
    PathEscape(String),
    #[error("document not found: {0}")]
    #[allow(dead_code)]
    NotFound(String),
    #[error("failed to watch filesystem: {0}")]
    Watcher(String),
    #[error("parse error: {0}")]
    #[allow(dead_code)]
    Parse(String),
    #[error("{0}")]
    Message(String),
}

/// Relativize `absolute` against `root`, returning a `/`-joined key.
/// Returns `Ok(".")` when equal.
pub fn relative_key(root: &Path, absolute: &Path) -> Result<String, WorkspaceIndexError> {
    let root = root
        .canonicalize()
        .map_err(WorkspaceIndexError::Io)?;
    let absolute = absolute
        .canonicalize()
        .map_err(WorkspaceIndexError::Io)?;

    if absolute == root {
        return Ok(".".to_string());
    }

    let rel = absolute
        .strip_prefix(&root)
        .map_err(|_| WorkspaceIndexError::PathEscape(absolute.display().to_string()))?;

    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => {
                return Err(WorkspaceIndexError::PathEscape(
                    absolute.display().to_string(),
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

/// Like `relative_key`, but tolerates non-existent `absolute` paths by
/// canonicalizing the parent directory and joining the file name. Used by
/// `rename_document` / `remove_document` where the path may already be gone.
pub fn relative_key_lenient(
    root: &Path,
    absolute: &Path,
) -> Result<String, WorkspaceIndexError> {
    if absolute.exists() {
        return relative_key(root, absolute);
    }
    let root = root
        .canonicalize()
        .map_err(WorkspaceIndexError::Io)?;
    let parent = absolute.parent().ok_or_else(|| {
        WorkspaceIndexError::Message(format!("invalid path: {}", absolute.display()))
    })?;
    let name = absolute.file_name().ok_or_else(|| {
        WorkspaceIndexError::Message(format!("invalid path: {}", absolute.display()))
    })?;
    let parent = parent
        .canonicalize()
        .map_err(WorkspaceIndexError::Io)?;
    let absolute = parent.join(name);

    if absolute == root {
        return Ok(".".to_string());
    }
    let rel = absolute
        .strip_prefix(&root)
        .map_err(|_| WorkspaceIndexError::PathEscape(absolute.display().to_string()))?;

    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => {
                return Err(WorkspaceIndexError::PathEscape(
                    absolute.display().to_string(),
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

/// Resolve `target` against the directory of `source_document` (a repo-relative
/// `/`-joined key), normalizing `.` / `..` components. Returns a repo-relative
/// key suitable for `DocumentId` lookup.
///
/// Empty `target` is returned unchanged (used by same-document `#anchor` xrefs).
/// Absolute-looking targets (`/…`) are returned with the leading slash stripped
/// so they still join under the repo root key space.
pub fn resolve_against_document(source_document: &str, target: &str) -> String {
    if target.is_empty() {
        return String::new();
    }

    let mut parts: Vec<String> = Vec::new();

    // Absolute-looking targets skip the source directory and start from root.
    let absolute_like = target.starts_with('/') || target.starts_with('\\');
    if !absolute_like {
        if let Some(parent) = Path::new(source_document).parent() {
            for component in parent.components() {
                if let Component::Normal(s) = component {
                    parts.push(s.to_string_lossy().into_owned());
                }
            }
        }
    }

    for part in target.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop();
            continue;
        }
        parts.push(part.to_string());
    }

    parts.join("/")
}

/// Resolve a `relative` path against `root`, rejecting `..` components.
#[allow(dead_code)]
pub fn join_relative(root: &Path, relative: &str) -> Result<PathBuf, WorkspaceIndexError> {
    if relative.is_empty() || relative == "." {
        return Ok(root.to_path_buf());
    }
    let mut out = root.to_path_buf();
    for part in relative.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(WorkspaceIndexError::PathEscape(relative.to_string()));
        }
        out.push(part);
    }
    Ok(out)
}

/// Convert `SystemTime` to a unix-seconds timestamp, defaulting to 0 on underflow.
pub fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("alfa-atlas-wi-domain-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn relative_key_round_trips() {
        let root = temp_dir();
        let nested = root.join("src").join("docs");
        fs::create_dir_all(&nested).unwrap();

        let key = relative_key(&root, &nested).unwrap();
        assert_eq!(key, "src/docs");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn join_relative_rejects_parent() {
        let root = temp_dir();
        assert!(join_relative(&root, "../outside").is_err());
        assert!(join_relative(&root, "a/b").is_ok());
    }

    #[test]
    fn resolve_against_document_normalizes_relative_includes() {
        assert_eq!(
            resolve_against_document("src/docs/asciidoc/index.adoc", "./_external/foo.adoc"),
            "src/docs/asciidoc/_external/foo.adoc"
        );
        assert_eq!(
            resolve_against_document(
                "src/docs/asciidoc/getBookkeepingServicesInfo/doc.adoc",
                "../_external/foo.adoc"
            ),
            "src/docs/asciidoc/_external/foo.adoc"
        );
        assert_eq!(
            resolve_against_document("src/docs/a.adoc", "sibling.adoc"),
            "src/docs/sibling.adoc"
        );
        assert_eq!(resolve_against_document("a.adoc", "b.adoc"), "b.adoc");
        assert_eq!(resolve_against_document("a.adoc", ""), "");
    }

    #[test]
    fn document_id_serde_transparent() {
        let id = DocumentId::new("docs/install.adoc");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"docs/install.adoc\"");
    }
}
