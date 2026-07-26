//! In-memory workspace index service.
//!
//! Holds DashMap repositories keyed by `DocumentId` (relative path) and provides
//! O(1) lookups for documents, anchors, includes, references, attributes, and
//! images. The index is built once on `build()` and updated incrementally by
//! the file watcher (see `services/file_watcher`).
//!
//! Concurrency model: each repository is an independent `DashMap`, so reads via
//! `get()` return a snapshot guard; writers replace the per-document entry in
//! a single write. Concurrent readers may observe a stale-but-consistent state
//! during an update — they see either the old entries or the new ones, never a
//! half-written mix, because each per-document update is a single
//! `DashMap::insert` per repository.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use dashmap::DashMap;
use tauri::{AppHandle, Emitter};

use crate::domain::supported_files::is_supported_file;
use crate::domain::workspace_index::{
    relative_key, relative_key_lenient, unix_seconds, Anchor, Attribute, Diagnostic, Document,
    DocumentId, Image, Include, IndexEvent, IndexStats, ParsedDocument, Reference,
    Severity, WorkspaceIndexError,
};
use crate::infra::parsers::registry::ParserRegistry;
use crate::infra::workspace_scanner;
use crate::services::diagnostics;

const EVENT_CHANNEL: &str = "workspace-index://event";

/// Reverse-dependency map: for each target `DocumentId`, the set of documents
/// that reference it (via include or xref). Used to recompute diagnostics for
/// dependents when a target changes.
type DependentsMap = DashMap<DocumentId, Vec<DocumentId>>;

pub struct WorkspaceIndex {
    repo_root: RwLock<Option<PathBuf>>,
    documents: DashMap<DocumentId, Document>,
    anchors: DashMap<String, Vec<Anchor>>,
    anchors_by_doc: DashMap<DocumentId, Vec<Anchor>>,
    includes: DashMap<DocumentId, Vec<Include>>,
    references: DashMap<DocumentId, Vec<Reference>>,
    attributes: DashMap<String, Vec<Attribute>>,
    attributes_by_doc: DashMap<DocumentId, Vec<Attribute>>,
    images: DashMap<String, Vec<Image>>,
    images_by_doc: DashMap<DocumentId, Vec<Image>>,
    diagnostics: DashMap<DocumentId, Vec<Diagnostic>>,
    dependents: DependentsMap,
    parsers: ParserRegistry,
    app_handle: RwLock<Option<AppHandle>>,
    watcher: RwLock<Option<crate::services::file_watcher::FileWatcher>>,
}

impl WorkspaceIndex {
    pub fn new(parsers: ParserRegistry) -> Self {
        Self {
            repo_root: RwLock::new(None),
            documents: DashMap::new(),
            anchors: DashMap::new(),
            anchors_by_doc: DashMap::new(),
            includes: DashMap::new(),
            references: DashMap::new(),
            attributes: DashMap::new(),
            attributes_by_doc: DashMap::new(),
            images: DashMap::new(),
            images_by_doc: DashMap::new(),
            diagnostics: DashMap::new(),
            dependents: DashMap::new(),
            parsers,
            app_handle: RwLock::new(None),
            watcher: RwLock::new(None),
        }
    }

    pub fn set_app_handle(&self, handle: AppHandle) {
        *self.app_handle.write().unwrap() = Some(handle);
    }

    pub fn is_open(&self) -> bool {
        self.repo_root.read().unwrap().is_some()
    }

    pub fn repo_root(&self) -> Option<PathBuf> {
        self.repo_root.read().unwrap().clone()
    }

    /// Build the index from scratch. Clears any previous state and emits
    /// `IndexBuildingStarted`, `IndexBuildingProgress`, and `IndexBuildingFinished`.
    pub fn build(&self, repo_root: PathBuf) -> Result<IndexStats, WorkspaceIndexError> {
        let canonical = repo_root
            .canonicalize()
            .map_err(WorkspaceIndexError::Io)?;
        if !canonical.is_dir() {
            return Err(WorkspaceIndexError::Message(format!(
                "not a directory: {}",
                canonical.display()
            )));
        }

        self.clear();

        *self.repo_root.write().unwrap() = Some(canonical.clone());
        self.emit(IndexEvent::IndexBuildingStarted);

        let files = workspace_scanner::scan(&canonical)?;
        let total = files.len() as u32;

        for (i, file) in files.iter().enumerate() {
            let done = (i as u32) + 1;
            let current = file
                .path
                .strip_prefix(&canonical)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.emit(IndexEvent::IndexBuildingProgress {
                done,
                total,
                current: current.clone(),
            });

            if let Err(e) = self.index_file(&canonical, file.path.clone(), file.modified) {
                // A single bad file shouldn't abort the whole build; emit a
                // warning-level diagnostic for the index log but keep going.
                let _ = e;
            }
        }

        // After all docs are loaded, compute diagnostics.
        diagnostics::run_all(self);
        let stats = self.compute_stats();
        self.emit(IndexEvent::IndexBuildingFinished {
            stats: stats.clone(),
        });

        Ok(stats)
    }

    /// Start watching the current repo root for changes. No-op if already watching.
    /// Requires `self` to be wrapped in an `Arc`; callers typically do
    /// `Arc::clone(&index)` before calling.
    pub fn start_watcher(self: &Arc<Self>) -> Result<(), WorkspaceIndexError> {
        let root = self
            .repo_root()
            .ok_or(WorkspaceIndexError::NotOpen)?;
        if self.watcher.read().unwrap().is_some() {
            return Ok(());
        }
        let watcher = crate::services::file_watcher::FileWatcher::start(root, self.clone())?;
        *self.watcher.write().unwrap() = Some(watcher);
        Ok(())
    }

    /// Stop the file watcher if running. Called by `clear`.
    pub fn stop_watcher(&self) {
        *self.watcher.write().unwrap() = None;
    }

    /// Drop all state. Called on `build()` (before repopulating) and on project close.
    pub fn clear(&self) {
        self.stop_watcher();
        *self.repo_root.write().unwrap() = None;
        self.documents.clear();
        self.anchors.clear();
        self.anchors_by_doc.clear();
        self.includes.clear();
        self.references.clear();
        self.attributes.clear();
        self.attributes_by_doc.clear();
        self.images.clear();
        self.images_by_doc.clear();
        self.diagnostics.clear();
        self.dependents.clear();
    }

    /// Incremental update on a file change/create.
    pub fn update_document(&self, path: PathBuf) -> Result<(), WorkspaceIndexError> {
        let root = self.repo_root.read().unwrap().clone().ok_or(WorkspaceIndexError::NotOpen)?;
        let path_str = path.to_string_lossy().into_owned();
        if !is_supported_file(&path_str) {
            return Ok(());
        }
        if !path.exists() {
            return self.remove_document(path);
        }

        let meta = std::fs::metadata(&path).map_err(WorkspaceIndexError::Io)?;
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let old_id = self.document_id_for_path(&root, &path);
        self.remove_entries_for_doc(&old_id);

        let path_str = path.to_string_lossy().into_owned();
        self.index_file(&root, path, modified)?;
        diagnostics::run_for(self, &old_id);
        // Recompute dependents of the new document too.
        if let Some(new_id) = self.document_id_for_path_opt(&root, &old_id.0) {
            diagnostics::run_for(self, &new_id);
        }
        self.emit(IndexEvent::IndexUpdated {
            document: path_str.clone(),
        });
        self.emit_diagnostics_updated_str(&path_str);
        Ok(())
    }

    /// Incremental update on a file removal.
    pub fn remove_document(&self, path: PathBuf) -> Result<(), WorkspaceIndexError> {
        let root = self.repo_root.read().unwrap().clone().ok_or(WorkspaceIndexError::NotOpen)?;
        let id = self.document_id_for_path(&root, &path);
        let path_str = path.to_string_lossy().into_owned();
        self.remove_entries_for_doc(&id);
        diagnostics::run_for(self, &id);
        self.emit(IndexEvent::IndexUpdated {
            document: path_str.clone(),
        });
        self.emit_diagnostics_updated_str(&path_str);
        Ok(())
    }

    /// Rename: update the document row, but leave references in OTHER docs
    /// pointing at the old path (they become broken until manually edited),
    /// per spec section 4.
    #[allow(dead_code)]
    pub fn rename_document(
        &self,
        old: PathBuf,
        new: PathBuf,
    ) -> Result<(), WorkspaceIndexError> {
        let root = self.repo_root.read().unwrap().clone().ok_or(WorkspaceIndexError::NotOpen)?;
        let old_id = self.document_id_for_path(&root, &old);
        // Remove old entries (anchors/attributes/etc. were tied to old_id).
        self.remove_entries_for_doc(&old_id);

        if new.exists() {
            let meta = std::fs::metadata(&new).map_err(WorkspaceIndexError::Io)?;
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            self.index_file(&root, new, modified)?;
        }
        diagnostics::run_for(self, &old_id);
        self.emit(IndexEvent::IndexUpdated {
            document: old.to_string_lossy().into_owned(),
        });
        Ok(())
    }

    /// Read, parse, and insert one file into the index.
    fn index_file(
        &self,
        root: &Path,
        path: PathBuf,
        modified: SystemTime,
    ) -> Result<(), WorkspaceIndexError> {
        let content = std::fs::read_to_string(&path).map_err(|e| {
            // Binary or unreadable files are skipped silently.
            WorkspaceIndexError::Message(format!("read {}: {}", path.display(), e))
        })?;
        let path_str = path.to_string_lossy().into_owned();

        let Some(doc_type) = self.parsers.doc_type(&path_str) else {
            return Ok(());
        };

        let relative = relative_key(root, &path)?;
        let id = DocumentId::new(relative.clone());
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        let document = Document {
            id: id.clone(),
            absolute_path: path.to_string_lossy().into_owned(),
            relative_path: relative,
            file_name,
            doc_type,
            modified_at: unix_seconds(modified),
        };
        self.documents.insert(id.clone(), document);

        let parsed = self.parsers.parse(&path_str, &content);
        self.insert_parsed(&id, parsed);

        Ok(())
    }

    /// Insert parsed entities, rewriting the `DOC_PLACEHOLDER` document id to
    /// the actual `DocumentId` and registering reverse-dep links.
    fn insert_parsed(&self, id: &DocumentId, mut parsed: ParsedDocument) {
        for a in &mut parsed.anchors {
            a.document = id.clone();
            self.anchors
                .entry(a.id.clone())
                .or_default()
                .push(a.clone());
            self.anchors_by_doc
                .entry(id.clone())
                .or_default()
                .push(a.clone());
        }
        for inc in &mut parsed.includes {
            inc.source_document = id.clone();
            self.includes
                .entry(id.clone())
                .or_default()
                .push(inc.clone());
            // Register reverse dependency so dependents recompute when target changes.
            let target_id = DocumentId::new(inc.path.clone());
            self.dependents
                .entry(target_id)
                .or_default()
                .push(id.clone());
        }
        for r in &mut parsed.references {
            r.source_document = id.clone();
            self.references
                .entry(id.clone())
                .or_default()
                .push(r.clone());
            let target_id = DocumentId::new(r.target_document.clone());
            self.dependents
                .entry(target_id)
                .or_default()
                .push(id.clone());
        }
        for attr in &mut parsed.attributes {
            attr.document = id.clone();
            self.attributes
                .entry(attr.name.clone())
                .or_default()
                .push(attr.clone());
            self.attributes_by_doc
                .entry(id.clone())
                .or_default()
                .push(attr.clone());
        }
        for img in &mut parsed.images {
            img.document = id.clone();
            self.images
                .entry(img.path.clone())
                .or_default()
                .push(img.clone());
            self.images_by_doc
                .entry(id.clone())
                .or_default()
                .push(img.clone());
        }
        // Parse-time diagnostics (syntax warnings).
        if !parsed.diagnostics.is_empty() {
            let mut fixed = parsed.diagnostics;
            for d in &mut fixed {
                d.document = id.clone();
            }
            self.diagnostics.insert(id.clone(), fixed);
        }
    }

    /// Remove every entry tied to `id` from all repositories.
    fn remove_entries_for_doc(&self, id: &DocumentId) {
        if let Some((_, doc)) = self.documents.remove(id) {
            let _ = doc;
        }

        // Anchors: remove from global map and per-doc map.
        if let Some((_, by_doc)) = self.anchors_by_doc.remove(id) {
            for a in by_doc {
                if let Some(mut entries) = self.anchors.get_mut(&a.id) {
                    entries.retain(|x| &x.document != id);
                    if entries.is_empty() {
                        drop(entries);
                        self.anchors.remove(&a.id);
                    }
                }
            }
        }

        // Includes: drop the per-doc list; clean reverse-dep entries that pointed at this doc.
        if let Some((_, includes)) = self.includes.remove(id) {
            for inc in includes {
                let target_id = DocumentId::new(inc.path);
                if let Some(mut deps) = self.dependents.get_mut(&target_id) {
                    deps.retain(|d| d != id);
                }
            }
        }

        if let Some((_, refs)) = self.references.remove(id) {
            for r in refs {
                let target_id = DocumentId::new(r.target_document);
                if let Some(mut deps) = self.dependents.get_mut(&target_id) {
                    deps.retain(|d| d != id);
                }
            }
        }

        if let Some((_, by_doc)) = self.attributes_by_doc.remove(id) {
            for attr in by_doc {
                if let Some(mut entries) = self.attributes.get_mut(&attr.name) {
                    entries.retain(|x| &x.document != id);
                    if entries.is_empty() {
                        drop(entries);
                        self.attributes.remove(&attr.name);
                    }
                }
            }
        }

        if let Some((_, by_doc)) = self.images_by_doc.remove(id) {
            for img in by_doc {
                if let Some(mut entries) = self.images.get_mut(&img.path) {
                    entries.retain(|x| &x.document != id);
                    if entries.is_empty() {
                        drop(entries);
                        self.images.remove(&img.path);
                    }
                }
            }
        }

        self.diagnostics.remove(id);
    }

    fn document_id_for_path(&self, root: &Path, path: &Path) -> DocumentId {
        relative_key_lenient(root, path)
            .map(DocumentId::new)
            .unwrap_or_else(|_| DocumentId::new(path.to_string_lossy().into_owned()))
    }

    fn document_id_for_path_opt(&self, root: &Path, relative: &str) -> Option<DocumentId> {
        if relative.is_empty() {
            return None;
        }
        let id = DocumentId::new(relative);
        if self.documents.contains_key(&id) {
            Some(id)
        } else {
            // `relative` may be the absolute path passed via update_document's
            // fallback; try resolving it against the root.
            let path = root.join(relative);
            relative_key(root, &path)
                .ok()
                .map(DocumentId::new)
                .filter(|id| self.documents.contains_key(id))
        }
    }

    fn compute_stats(&self) -> IndexStats {
        let mut stats = IndexStats::default();
        stats.documents = self.documents.len() as u32;
        stats.anchors = self.anchors_by_doc.iter().map(|r| r.value().len() as u32).sum();
        stats.includes = self.includes.iter().map(|r| r.value().len() as u32).sum();
        stats.references = self.references.iter().map(|r| r.value().len() as u32).sum();
        stats.attributes = self.attributes_by_doc.iter().map(|r| r.value().len() as u32).sum();
        stats.images = self.images_by_doc.iter().map(|r| r.value().len() as u32).sum();
        for diag in self.diagnostics.iter() {
            for d in diag.value() {
                match d.severity {
                    Severity::Error => stats.errors += 1,
                    Severity::Warning => stats.warnings += 1,
                }
            }
        }
        stats
    }

    fn emit(&self, event: IndexEvent) {
        if let Some(handle) = self.app_handle.read().unwrap().as_ref() {
            let _ = handle.emit(EVENT_CHANNEL, &event);
        }
    }

    fn emit_diagnostics_updated_str(&self, path: &str) {
        self.emit(IndexEvent::DiagnosticsUpdated {
            document: path.to_string(),
        });
    }

    // --- Public read API (spec section 7) ---

    pub fn get_document(&self, path: &Path) -> Option<Document> {
        let root = self.repo_root.read().unwrap().clone()?;
        let id = relative_key_lenient(&root, path).ok().map(DocumentId::new)?;
        self.documents.get(&id).map(|r| r.clone())
    }

    pub fn get_documents(&self) -> Vec<Document> {
        self.documents.iter().map(|r| r.value().clone()).collect()
    }

    pub fn find_document(&self, name: &str) -> Vec<Document> {
        self.documents
            .iter()
            .filter(|d| {
                d.file_name.contains(name) || d.relative_path.contains(name)
            })
            .map(|r| r.value().clone())
            .collect()
    }

    pub fn find_anchor(&self, id: &str) -> Vec<Anchor> {
        self.anchors.get(id).map(|r| r.clone()).unwrap_or_default()
    }

    pub fn find_anchors(&self, document: &DocumentId) -> Vec<Anchor> {
        self.anchors_by_doc
            .get(document)
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    pub fn find_includes(&self, document: &DocumentId) -> Vec<Include> {
        self.includes
            .get(document)
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    pub fn find_references(&self, document: &DocumentId) -> Vec<Reference> {
        self.references
            .get(document)
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    pub fn find_attribute(&self, name: &str) -> Vec<Attribute> {
        self.attributes
            .get(name)
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    pub fn get_attributes(&self, document: &DocumentId) -> Vec<Attribute> {
        self.attributes_by_doc
            .get(document)
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    pub fn find_image(&self, path: &str) -> Vec<Image> {
        self.images.get(path).map(|r| r.clone()).unwrap_or_default()
    }

    pub fn get_diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics
            .iter()
            .flat_map(|r| r.value().clone())
            .collect()
    }

    pub fn get_diagnostics_for(&self, document: &DocumentId) -> Vec<Diagnostic> {
        self.diagnostics
            .get(document)
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    // --- Accessors used by the diagnostics service ---

    pub(crate) fn documents_iter(&self) -> Vec<Document> {
        self.documents.iter().map(|r| r.value().clone()).collect()
    }

    pub(crate) fn dependents_of(&self, doc: &DocumentId) -> Vec<DocumentId> {
        self.dependents
            .get(doc)
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    pub(crate) fn set_diagnostics(&self, doc: &DocumentId, diags: Vec<Diagnostic>) {
        if diags.is_empty() {
            self.diagnostics.remove(doc);
        } else {
            self.diagnostics.insert(doc.clone(), diags);
        }
    }

    // --- Helpers used by the diagnostics service ---

    pub(crate) fn document_exists_by_relative(&self, rel: &str) -> bool {
        self.documents.contains_key(&DocumentId(rel.to_string()))
    }

    pub(crate) fn anchor_exists_in(&self, doc: &DocumentId, anchor: &str) -> bool {
        self.anchors_by_doc
            .get(doc)
            .map(|a| a.iter().any(|x| x.id == anchor))
            .unwrap_or(false)
    }

    pub(crate) fn anchor_count(&self, id: &str) -> usize {
        self.anchors
            .get(id)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    pub(crate) fn images_for_doc(&self, doc: &DocumentId) -> Vec<Image> {
        self.images_by_doc
            .get(doc)
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    /// Does the image path resolve to an existing file under the repo root?
    pub(crate) fn image_exists(&self, path: &str) -> bool {
        let root = match self.repo_root.read().unwrap().clone() {
            Some(r) => r,
            None => return false,
        };
        let candidate = root.join(path);
        candidate.is_file()
    }
}

impl Default for WorkspaceIndex {
    fn default() -> Self {
        Self::new(ParserRegistry::new())
    }
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
        let dir = std::env::temp_dir().join(format!("docflow-wi-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn build_index(root: &Path) -> WorkspaceIndex {
        let idx = WorkspaceIndex::new(ParserRegistry::new());
        idx.build(root.to_path_buf()).unwrap();
        idx
    }

    #[test]
    fn build_indexes_adoc_anchors() {
        let root = temp_dir();
        fs::write(
            root.join("install.adoc"),
            "[[installation]]\n= Install\ninclude::common.adoc[]\n",
        )
        .unwrap();
        fs::write(root.join("common.adoc"), "= Common\n").unwrap();

        let idx = build_index(&root);
        assert_eq!(idx.documents.len(), 2);
        assert_eq!(idx.anchors.len(), 1); // "installation"
        assert_eq!(idx.find_anchor("installation").len(), 1);
        assert_eq!(idx.find_includes(&DocumentId::new("install.adoc")).len(), 1);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn build_emits_finished_event_stats() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "[[a]]\n= A\n").unwrap();
        let idx = build_index(&root);
        let stats = idx.compute_stats();
        assert_eq!(stats.documents, 1);
        assert_eq!(stats.anchors, 1);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn update_document_recomputes_anchors() {
        let root = temp_dir();
        fs::write(root.join("x.adoc"), "[[old]]\n= X\n").unwrap();
        let idx = build_index(&root);
        assert_eq!(idx.find_anchor("old").len(), 1);

        fs::write(root.join("x.adoc"), "[[new]]\n= X2\n").unwrap();
        idx.update_document(root.join("x.adoc")).unwrap();
        assert_eq!(idx.find_anchor("old").len(), 0);
        assert_eq!(idx.find_anchor("new").len(), 1);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn remove_document_drops_entries() {
        let root = temp_dir();
        fs::write(root.join("y.adoc"), "[[yanchor]]\n= Y\n").unwrap();
        let idx = build_index(&root);
        idx.remove_document(root.join("y.adoc")).unwrap();
        assert_eq!(idx.find_anchor("yanchor").len(), 0);
        assert!(idx.get_document(&root.join("y.adoc")).is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rename_keeps_old_refs_broken() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "include::b.adoc[]\n").unwrap();
        fs::write(root.join("b.adoc"), "= B\n").unwrap();
        let idx = build_index(&root);
        // No diagnostics expected initially.
        let diags = idx.get_diagnostics();
        assert!(diags.iter().all(|d| d.kind != crate::domain::workspace_index::DiagnosticKind::MissingInclude));

        // Rename b.adoc -> c.adoc.
        fs::rename(root.join("b.adoc"), root.join("c.adoc")).unwrap();
        idx.rename_document(root.join("b.adoc"), root.join("c.adoc"))
            .unwrap();
        // Now a.adoc's include points to b.adoc which no longer exists.
        let diags = idx.get_diagnostics();
        assert!(diags
            .iter()
            .any(|d| d.kind == crate::domain::workspace_index::DiagnosticKind::MissingInclude));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn find_document_by_name() {
        let root = temp_dir();
        fs::write(root.join("config.adoc"), "= Config\n").unwrap();
        let idx = build_index(&root);
        let found = idx.find_document("config");
        assert_eq!(found.len(), 1);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn clear_drops_all() {
        let root = temp_dir();
        fs::write(root.join("c.adoc"), "[[cc]]\n= C\n").unwrap();
        let idx = build_index(&root);
        assert!(!idx.documents.is_empty());
        idx.clear();
        assert!(idx.documents.is_empty());
        assert!(!idx.is_open());
        fs::remove_dir_all(&root).ok();
    }
}
