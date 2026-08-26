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

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::SystemTime;

use dashmap::DashMap;

use crate::domain::asciidoc_facts::{
    AsciiDocFacts, AsciiDocParseRequested, ParseErrorFact,
};
use crate::domain::supported_files::is_supported_file;
use crate::domain::workspace_index::{
    relative_key, relative_key_lenient, resolve_against_document, unix_seconds, Anchor, Attribute,
    Diagnostic, DiagnosticKind, Document, DocumentId, DocumentType, Image, Include, IndexEvent,
    IndexStats, ParsedDocument, Reference, Severity, WorkspaceIndexError, WorkspaceIndexEvent,
    WorkspaceIndexEventSink,
};
use crate::infra::parsers::registry::ParserRegistry;
use crate::infra::workspace_scanner;
use crate::services::diagnostics;

const PARSE_TIMEOUT_SECS: u64 = 30;

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
    /// `None` until the command layer installs one — the index still
    /// works headless (tests do exactly that), it just reports nowhere.
    event_sink: RwLock<Option<WorkspaceIndexEventSink>>,
    watcher: RwLock<Option<crate::services::file_watcher::FileWatcher>>,

    // --- AsciiDoc async coordinator state ---
    /// Monotonic per-document version counter; incremented on each dispatch
    /// for the document, removed on document deletion.
    doc_versions: DashMap<DocumentId, u64>,
    /// Pending parse requests buffered while the frontend is not yet ready
    /// or while `inflight_adoc_count` is at `max_inflight`.
    pending_adoc_queue: RwLock<VecDeque<AsciiDocParseRequested>>,
    /// `AbortHandle` for the timeout task of each in-flight parse, keyed by
    /// (document_id, version) so concurrent dispatches for the same document
    /// do not interfere with each other. The value is the `JoinHandle` itself
    /// (Tauri's wrapper over tokio's), which exposes `.abort()` directly.
    parse_timeouts: DashMap<(DocumentId, u64), tauri::async_runtime::JoinHandle<()>>,
    /// Number of parse requests currently in flight to the frontend.
    inflight_adoc_count: AtomicU32,
    /// Maximum number of concurrent in-flight parse requests. A field rather
    /// than a const so tests can inject a smaller value.
    max_inflight: u32,
    /// Counter of pending AsciiDoc facts during an initial build. Decremented
    /// as facts arrive; when it reaches zero, `IndexBuildingFinished` is
    /// emitted by `try_finish_build`.
    build_adoc_pending: AtomicU32,
    /// True between `IndexBuildingStarted` and `IndexBuildingFinished`. Used
    /// to defer the finished event until all AsciiDoc facts have arrived.
    building_in_progress: AtomicBool,
    /// Set to true once the frontend signals it is ready to receive parse
    /// requests. Before this is true, all dispatches are buffered.
    frontend_ready: AtomicBool,
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
            event_sink: RwLock::new(None),
            watcher: RwLock::new(None),
            doc_versions: DashMap::new(),
            pending_adoc_queue: RwLock::new(VecDeque::new()),
            parse_timeouts: DashMap::new(),
            inflight_adoc_count: AtomicU32::new(0),
            max_inflight: 8,
            build_adoc_pending: AtomicU32::new(0),
            building_in_progress: AtomicBool::new(false),
            frontend_ready: AtomicBool::new(false),
        }
    }

    /// Test-only constructor that allows injecting a smaller `max_inflight`
    /// to verify queue/overflow behavior without dispatching 8 concurrent
    /// parses.
    #[cfg(test)]
    pub fn with_max_inflight(parsers: ParserRegistry, max_inflight: u32) -> Self {
        let mut idx = Self::new(parsers);
        idx.max_inflight = max_inflight;
        idx
    }

    pub fn set_event_sink(&self, sink: WorkspaceIndexEventSink) {
        *self.event_sink_write() = Some(sink);
    }

    // --- Lock accessors ---
    //
    // Every `RwLock` here is read/written through these rather than
    // `.unwrap()` on the guard. What each lock protects is a plain value —
    // a path, a sink, a watcher handle, a queue — so a panic while one is
    // held leaves no torn state behind; only the mutual-exclusion property
    // matters, and that survives the unwind intact. Propagating
    // `PoisonError` instead would mean one panic anywhere disables the
    // whole workspace index for the rest of the process's life. Same policy
    // and same reasoning as `services::embedding_state::lock_sync_guard`.

    fn repo_root_read(&self) -> RwLockReadGuard<'_, Option<PathBuf>> {
        self.repo_root.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn repo_root_write(&self) -> RwLockWriteGuard<'_, Option<PathBuf>> {
        self.repo_root.write().unwrap_or_else(PoisonError::into_inner)
    }

    fn event_sink_read(&self) -> RwLockReadGuard<'_, Option<WorkspaceIndexEventSink>> {
        self.event_sink.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn event_sink_write(&self) -> RwLockWriteGuard<'_, Option<WorkspaceIndexEventSink>> {
        self.event_sink.write().unwrap_or_else(PoisonError::into_inner)
    }

    fn watcher_read(&self) -> RwLockReadGuard<'_, Option<crate::services::file_watcher::FileWatcher>> {
        self.watcher.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn watcher_write(&self) -> RwLockWriteGuard<'_, Option<crate::services::file_watcher::FileWatcher>> {
        self.watcher.write().unwrap_or_else(PoisonError::into_inner)
    }

    fn adoc_queue_write(&self) -> RwLockWriteGuard<'_, VecDeque<AsciiDocParseRequested>> {
        self.pending_adoc_queue.write().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn is_open(&self) -> bool {
        self.repo_root_read().is_some()
    }

    pub fn repo_root(&self) -> Option<PathBuf> {
        self.repo_root_read().clone()
    }

    /// Build the index from scratch. Clears any previous state and emits
    /// `IndexBuildingStarted`, `IndexBuildingProgress`, and `IndexBuildingFinished`.
    ///
    /// Takes `&Arc<Self>` so that the AsciiDoc async delegation path can clone
    /// the `Arc` into the timeout task. The non-AsciiDoc / test path does not
    /// need it.
    pub fn build(self: &Arc<Self>, repo_root: PathBuf) -> Result<IndexStats, WorkspaceIndexError> {
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

        *self.repo_root_write() = Some(canonical.clone());
        self.emit(IndexEvent::IndexBuildingStarted);
        self.building_in_progress.store(true, Ordering::SeqCst);

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

            if let Err(e) = self.index_file(
                Arc::clone(self),
                &canonical,
                file.path.clone(),
                file.modified,
            ) {
                // A single bad file shouldn't abort the whole build; emit a
                // warning-level diagnostic for the index log but keep going.
                let _ = e;
            }
        }

        // If no AsciiDoc facts are pending (e.g., no .adoc files, or all
        // already processed synchronously in tests), finish immediately.
        // Otherwise, `IndexBuildingFinished` is emitted by `try_finish_build`
        // when the last fact arrives.
        let adoc_pending = self.build_adoc_pending.load(Ordering::SeqCst);
        if adoc_pending == 0 {
            diagnostics::run_all(self);
            let stats = self.compute_stats();
            self.building_in_progress.store(false, Ordering::SeqCst);
            self.emit(IndexEvent::IndexBuildingFinished {
                stats: stats.clone(),
            });
            Ok(stats)
        } else {
            // IndexBuildingFinished will be emitted by try_finish_build()
            // when the last fact arrives. The status bar will keep showing
            // "building" until then.
            Ok(IndexStats::default())
        }
    }

    /// Start watching the current repo root for changes. No-op if already watching.
    /// Requires `self` to be wrapped in an `Arc`; callers typically do
    /// `Arc::clone(&index)` before calling.
    pub fn start_watcher(self: &Arc<Self>) -> Result<(), WorkspaceIndexError> {
        let root = self
            .repo_root()
            .ok_or(WorkspaceIndexError::NotOpen)?;
        if self.watcher_read().is_some() {
            return Ok(());
        }
        let watcher = crate::services::file_watcher::FileWatcher::start(root, self.clone())?;
        *self.watcher_write() = Some(watcher);
        Ok(())
    }

    /// Stop the file watcher if running. Called by `clear`.
    pub fn stop_watcher(&self) {
        *self.watcher_write() = None;
    }

    /// Drop all state. Called on `build()` (before repopulating) and on project close.
    pub fn clear(&self) {
        self.stop_watcher();
        *self.repo_root_write() = None;
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

        // Reset coordinator state so a fresh build starts from a clean slate.
        // Abort in-flight timeout tasks so they cannot fire against the next build.
        self.parse_timeouts.retain(|_, handle| {
            handle.abort();
            false
        });
        self.doc_versions.clear();
        *self.adoc_queue_write() = VecDeque::new();
        self.inflight_adoc_count.store(0, Ordering::SeqCst);
        self.build_adoc_pending.store(0, Ordering::SeqCst);
        self.building_in_progress.store(false, Ordering::SeqCst);
        // Do NOT reset `frontend_ready`: the React listener stays mounted across
        // rebuilds. Resetting it here would queue every parse request with no
        // subsequent `frontend_ready` call to drain them.
    }

    /// Incremental update on a file change/create.
    pub fn update_document(self: &Arc<Self>, path: PathBuf) -> Result<(), WorkspaceIndexError> {
        let root = self.repo_root_read().clone().ok_or(WorkspaceIndexError::NotOpen)?;
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
        self.index_file(Arc::clone(self), &root, path, modified)?;
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
        let root = self.repo_root_read().clone().ok_or(WorkspaceIndexError::NotOpen)?;
        let id = self.document_id_for_path(&root, &path);
        let path_str = path.to_string_lossy().into_owned();
        self.remove_entries_for_doc(&id);
        // Drop the version entry so any in-flight parse for this doc is treated
        // as stale when its response arrives.
        self.doc_versions.remove(&id);
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
    pub fn rename_document(
        self: &Arc<Self>,
        old: PathBuf,
        new: PathBuf,
    ) -> Result<(), WorkspaceIndexError> {
        let root = self.repo_root_read().clone().ok_or(WorkspaceIndexError::NotOpen)?;
        let old_id = self.document_id_for_path(&root, &old);
        // Remove old entries (anchors/attributes/etc. were tied to old_id).
        self.remove_entries_for_doc(&old_id);
        self.doc_versions.remove(&old_id);

        if new.exists() {
            let meta = std::fs::metadata(&new).map_err(WorkspaceIndexError::Io)?;
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            self.index_file(Arc::clone(self), &root, new, modified)?;
        }
        diagnostics::run_for(self, &old_id);
        self.emit(IndexEvent::IndexUpdated {
            document: old.to_string_lossy().into_owned(),
        });
        Ok(())
    }

    /// Read, parse, and insert one file into the index.
    ///
    /// `self_arc` is the owning `Arc` of this `WorkspaceIndex`. It is needed
    /// because the AsciiDoc path spawns a timeout task that holds an `Arc`
    /// reference. For the non-AsciiDoc / test path, `self_arc` is unused.
    fn index_file(
        &self,
        self_arc: Arc<Self>,
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
            relative_path: relative.clone(),
            file_name,
            doc_type,
            modified_at: unix_seconds(modified),
        };
        self.documents.insert(id.clone(), document);

        if doc_type == DocumentType::AsciiDoc && self.event_sink_read().is_some() {
            // Production: delegate AsciiDoc parsing to the frontend.
            self_arc.dispatch_asciidoc_parse(&id, content, relative);
        } else {
            // Tests or non-AsciiDoc: synchronous parser.
            let parsed = self.parsers.parse(&path_str, &content);
            self.insert_parsed(&id, parsed);
        }

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
            // Normalize include targets to repo-relative keys so diagnostics
            // can look them up by DocumentId (e.g. `../_external/foo.adoc`
            // from `src/docs/a.adoc` → `src/docs/_external/foo.adoc`).
            inc.path = resolve_against_document(&id.0, &inc.path);
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
            if !r.target_document.is_empty() {
                r.target_document = resolve_against_document(&id.0, &r.target_document);
            }
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
            img.path = resolve_against_document(&id.0, &img.path);
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

    /// Remove the document row and every fact entry tied to `id`.
    /// Used when a file is deleted/renamed/replaced.
    fn remove_entries_for_doc(&self, id: &DocumentId) {
        self.documents.remove(id);
        self.clear_facts_for_doc(id);
    }

    /// Clear fact repositories for `id` but keep the `Document` row.
    /// Used when re-applying AsciiDoc facts from the frontend — the document
    /// was already registered in `index_file` and must remain visible so
    /// other documents' include/xref diagnostics can resolve against it.
    fn clear_facts_for_doc(&self, id: &DocumentId) {
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
        self.report(WorkspaceIndexEvent::Index(event));
    }

    fn report(&self, event: WorkspaceIndexEvent) {
        if let Some(sink) = self.event_sink_read().as_ref() {
            sink(event);
        }
    }

    fn emit_diagnostics_updated_str(&self, path: &str) {
        self.emit(IndexEvent::DiagnosticsUpdated {
            document: path.to_string(),
        });
    }

    // --- AsciiDoc async coordinator ---

    fn try_emit_parse_request(&self, payload: AsciiDocParseRequested) {
        self.report(WorkspaceIndexEvent::AsciiDocParseRequested(payload));
    }

    /// Dispatch a parse request for `doc_id` to the frontend.
    ///
    /// Increments the per-document version counter, registers a timeout task,
    /// and either emits the request immediately (if a slot is available) or
    /// buffers it in `pending_adoc_queue`. Also increments `build_adoc_pending`
    /// when a build is in progress, so `try_finish_build` knows when the last
    /// fact has arrived.
    fn dispatch_asciidoc_parse(
        self: &Arc<Self>,
        doc_id: &DocumentId,
        content: String,
        relative_path: String,
    ) {
        let version = *self
            .doc_versions
            .entry(doc_id.clone())
            .and_modify(|v| *v += 1)
            .or_insert(1);

        let payload = AsciiDocParseRequested {
            document_id: doc_id.clone(),
            version,
            content,
            relative_path: PathBuf::from(&relative_path),
        };

        // Spawn a timeout task so a hung frontend cannot stall the queue.
        let doc_id_for_timeout = doc_id.clone();
        let arc_for_timeout = Arc::clone(self);
        let handle = tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(PARSE_TIMEOUT_SECS)).await;
            arc_for_timeout.handle_parse_timeout(&doc_id_for_timeout, version);
        });
        self.parse_timeouts
            .insert((doc_id.clone(), version), handle);

        if !self.frontend_ready.load(Ordering::SeqCst) {
            self.adoc_queue_write().push_back(payload);
            if self.building_in_progress.load(Ordering::SeqCst) {
                self.build_adoc_pending.fetch_add(1, Ordering::SeqCst);
            }
            return;
        }

        let current = self.inflight_adoc_count.load(Ordering::SeqCst);
        if current < self.max_inflight {
            self.inflight_adoc_count.fetch_add(1, Ordering::SeqCst);
            self.try_emit_parse_request(payload);
        } else {
            self.adoc_queue_write().push_back(payload);
        }
        if self.building_in_progress.load(Ordering::SeqCst) {
            self.build_adoc_pending.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Frontend signals it is ready to receive parse requests. Drains the
    /// buffered queue up to `max_inflight`.
    pub fn frontend_ready(&self) {
        self.frontend_ready.store(true, Ordering::SeqCst);
        let mut queue = self.adoc_queue_write();
        loop {
            let current = self.inflight_adoc_count.load(Ordering::SeqCst);
            if current >= self.max_inflight {
                break;
            }
            let next = queue.pop_front();
            // Release the lock before dispatch — `next` is owned and the
            // dispatch path does not need the queue.
            drop(queue);
            if let Some(payload) = next {
                self.inflight_adoc_count.fetch_add(1, Ordering::SeqCst);
                self.try_emit_parse_request(payload);
                queue = self.adoc_queue_write();
            } else {
                break;
            }
        }
    }

    /// Receive parsed facts from the frontend.
    ///
    /// Bookkeeping (decrement `inflight_adoc_count`, drain queue,
    /// `try_finish_build`) runs unconditionally — even for stale responses —
    /// so a stale response cannot leak the counter or hang the build.
    pub fn submit_asciidoc_facts(
        &self,
        doc_id: &DocumentId,
        version: u64,
        facts: AsciiDocFacts,
    ) -> Result<(), WorkspaceIndexError> {
        // Abort the timeout for this specific (document, version) pair.
        // Guarantees exactly-once execution: if the response arrives before
        // the timeout fires, the timeout is cancelled. If the timeout fired
        // first, its entry is already removed and this remove returns None.
        if let Some((_, handle)) = self.parse_timeouts.remove(&(doc_id.clone(), version)) {
            handle.abort();
        }

        let is_valid = match self.doc_versions.get(doc_id) {
            Some(current) => *current == version,
            None => false,
        };

        if is_valid {
            let parsed = self.facts_to_parsed(doc_id, &facts);
            // Keep the Document row — only replace facts. Removing the document
            // would make every include/xref targeting this file look missing.
            self.clear_facts_for_doc(doc_id);
            self.insert_parsed(doc_id, parsed);

            // Recompute diagnostics for this doc AND its dependents (run_for
            // does BFS over the dependents graph). A "missing xref anchor"
            // in another document may resolve once this document's facts
            // arrive. This also sets the canonical diagnostics for `doc_id`.
            diagnostics::run_for(self, doc_id);

            // Parse errors are added AFTER run_for so they survive — they are
            // orthogonal to the cross-document diagnostics run_for computes
            // (missing includes, broken xrefs, duplicate anchors, etc.).
            if !facts.parse_errors.is_empty() {
                let mut diags = self.get_diagnostics_for(doc_id);
                for e in &facts.parse_errors {
                    diags.push(Diagnostic {
                        kind: DiagnosticKind::ParseError,
                        message: e.message.clone(),
                        document: doc_id.clone(),
                        line: e.line.unwrap_or(1),
                        column: 1,
                        severity: if e.severity.eq_ignore_ascii_case("error") {
                            Severity::Error
                        } else {
                            Severity::Warning
                        },
                    });
                }
                self.set_diagnostics(doc_id, diags);
            }

            self.emit(IndexEvent::IndexUpdated {
                document: doc_id.0.clone(),
            });
            self.emit(IndexEvent::DiagnosticsUpdated {
                document: doc_id.0.clone(),
            });
        }

        // --- Bookkeeping: runs unconditionally. ---

        self.inflight_adoc_count.fetch_sub(1, Ordering::SeqCst);

        // Drain queue. Release the lock before dispatch — `next` is owned.
        let next = self.adoc_queue_write().pop_front();
        if let Some(payload) = next {
            self.inflight_adoc_count.fetch_add(1, Ordering::SeqCst);
            self.try_emit_parse_request(payload);
        }

        self.try_finish_build();

        Ok(())
    }

    fn handle_parse_timeout(&self, doc_id: &DocumentId, version: u64) {
        // Remove our own entry first. If a real response already arrived and
        // aborted this handle, the remove returns None — that's fine.
        self.parse_timeouts.remove(&(doc_id.clone(), version));

        let is_current = match self.doc_versions.get(doc_id) {
            Some(v) => *v == version,
            None => false,
        };
        if !is_current {
            return;
        }
        // Synthesize a parse error and run through the standard path so the
        // queue drains and the build can finish.
        let lang = crate::infra::settings_store::load()
            .map(|s| s.general.error_language)
            .unwrap_or_default();
        let facts = AsciiDocFacts {
            anchors: vec![],
            includes: vec![],
            references: vec![],
            attributes: vec![],
            images: vec![],
            parse_errors: vec![ParseErrorFact {
                message: crate::services::diagnostic_messages::parse_timeout(
                    lang,
                    PARSE_TIMEOUT_SECS,
                ),
                line: None,
                severity: "error".to_string(),
            }],
        };
        let _ = self.submit_asciidoc_facts(doc_id, version, facts);
    }

    fn try_finish_build(&self) {
        if !self.building_in_progress.load(Ordering::SeqCst) {
            return;
        }
        let pending = self.build_adoc_pending.fetch_sub(1, Ordering::SeqCst);
        if pending == 1 {
            // Last pending adoc fact arrived. Run a full diagnostics pass to
            // catch cross-document issues (e.g., DuplicateAnchor) that
            // per-doc `run_for` would not have caught, then emit the final
            // event with complete stats.
            diagnostics::run_all(self);
            let stats = self.compute_stats();
            self.building_in_progress.store(false, Ordering::SeqCst);
            self.emit(IndexEvent::IndexBuildingFinished { stats });
        }
    }

    /// Convert frontend facts into a `ParsedDocument` with the `document` /
    /// `source_document` fields filled in from `doc_id`.
    fn facts_to_parsed(&self, doc_id: &DocumentId, facts: &AsciiDocFacts) -> ParsedDocument {
        let mut out = ParsedDocument::default();
        for a in &facts.anchors {
            out.anchors.push(Anchor {
                id: a.id.clone(),
                document: doc_id.clone(),
                line: a.line,
                column: a.column,
            });
        }
        for inc in &facts.includes {
            out.includes.push(Include {
                path: inc.path.clone(),
                source_document: doc_id.clone(),
                line: inc.line,
                column: inc.column,
            });
        }
        for r in &facts.references {
            out.references.push(Reference {
                target_document: r.target_document.clone(),
                anchor: r.anchor.clone(),
                source_document: doc_id.clone(),
                line: r.line,
                column: r.column,
            });
        }
        for attr in &facts.attributes {
            out.attributes.push(Attribute {
                name: attr.name.clone(),
                value: attr.value.clone(),
                document: doc_id.clone(),
                line: attr.line,
            });
        }
        for img in &facts.images {
            out.images.push(Image {
                path: img.path.clone(),
                document: doc_id.clone(),
                line: img.line,
            });
        }
        out
    }


    // --- Public read API (spec section 7) ---

    pub fn get_document(&self, path: &Path) -> Option<Document> {
        let root = self.repo_root_read().clone()?;
        let id = relative_key_lenient(&root, path).ok().map(DocumentId::new)?;
        self.documents.get(&id).map(|r| r.clone())
    }

    /// Same lookup as `get_document`, but by the document's already-known
    /// repo-relative `DocumentId` key directly — no filesystem
    /// canonicalize, so (unlike `get_document`) this works from a frontend
    /// that only ever deals in repo-relative path strings and has no
    /// absolute filesystem path to pass in. Same pattern `find_anchors`/
    /// `find_includes`/`find_references` already use for an exact-key
    /// lookup.
    pub fn get_document_by_id(&self, id: &str) -> Option<Document> {
        self.documents.get(&DocumentId::new(id.to_string())).map(|r| r.clone())
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
        let root = match self.repo_root_read().clone() {
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Several tests in this module call this concurrently. A nanosecond
    /// timestamp alone does not reliably disambiguate them on a coarser
    /// system clock — two would share a directory and clobber each other.
    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("alfa-atlas-wi-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn build_index(root: &Path) -> Arc<WorkspaceIndex> {
        let idx = Arc::new(WorkspaceIndex::new(ParserRegistry::new()));
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
    fn build_indexes_json_ref_as_an_include() {
        let root = temp_dir();
        fs::write(
            root.join("api.json"),
            r#"{"components": {"$ref": "./common.json"}}"#,
        )
        .unwrap();
        fs::write(root.join("common.json"), "{}").unwrap();

        let idx = build_index(&root);
        assert_eq!(idx.documents.len(), 2);
        let includes = idx.find_includes(&DocumentId::new("api.json"));
        assert_eq!(includes.len(), 1);
        // `insert_parsed` resolves the raw `./common.json` target to a
        // repo-relative key before storing it.
        assert_eq!(includes[0].path, "common.json");
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

#[cfg(test)]
#[path = "tests_asciidoc_coordinator.rs"]
mod tests_asciidoc_coordinator;
