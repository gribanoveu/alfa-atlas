---
name: replace-asciidoc-parser
overview: Replace the inline Rust AsciiDoc line/regex parser with asciidoctor.js running in the frontend, communicating via async Tauri IPC with versioning to prevent stale results.
todos:
  - id: domain-add-parse-error
    content: Add DiagnosticKind::ParseError to domain enum and frontend TypeScript type
    status: completed
  - id: domain-add-ipc-types
    content: Add AsciiDoc IPC contract types (AsciiDocParseRequested, AsciiDocFacts, fact structs) to domain module
    status: completed
  - id: index-version-tracking
    content: Add document version tracking (DashMap<DocumentId, u64>) to WorkspaceIndex + queue/state fields + reset in build()/clear()
    status: completed
  - id: index-index-file-branch
    content: Branch index_file() to delegate AsciiDoc parsing when app_handle is Some, keep sync fallback via ParserRegistry for tests
    status: completed
  - id: index-async-flow
    content: Implement dispatch_asciidoc_parse(), submit_asciidoc_facts(), frontend_ready(), queue with MAX_INFLIGHT+timeout, stale-version discard, build-pending counter for deferred IndexBuildingFinished
    status: completed
  - id: index-run-for-scope
    content: Verify/ensure submit_asciidoc_facts calls diagnostics::run_for on both the doc and its dependents, and after build emit run_all to cover cross-document diagnostics
    status: completed
  - id: commands-new
    content: Create commands/asciidoc.rs with submit_asciidoc_facts and frontend_ready commands
    status: completed
  - id: register-commands
    content: Register new commands in lib.rs generate_handler![] (Tauri v2 auto-generates capabilities — no JSON changes needed for custom commands)
    status: completed
  - id: frontend-deps
    content: Add asciidoctor npm dependency, verify IncludeProcessor API signature against docs, create src/lib/asciidocParser.ts wrapper
    status: completed
  - id: frontend-hook
    content: Create useAsciiDocParser hook with Asciidoctor.load(), AST walking, try/catch around full IPC round-trip (not just extractFacts)
    status: completed
  - id: frontend-mount-hook
    content: Mount useAsciiDocParser() in App.tsx
    status: completed
  - id: coordinator-tests
    content: Add unit tests for submit_asciidoc_facts stale-version discard, queue overflow, drain after frontend_ready
    status: completed
  - id: frontend-tests
    content: Add unit tests for extractFacts() with sample adoc content (port test cases from old Rust parser)
    status: completed
  - id: verify
    content: Run cargo check, tsc --noEmit, cargo test to verify everything compiles and all tests pass
    status: completed
isProject: false
---

# Replace Inline Rust AsciiDoc Parser with asciidoctor.js

## Overview

The current `ParserRegistry` dispatches `*.adoc` files to a lightweight line/regex parser in Rust (`infa/parsers/ascii_doc.rs`). This parser misses `ifdef`/`ifndef`, nested includes with `leveloffset`, and attribute substitution. The new flow delegates semantic parsing to `asciidoctor.js` in the frontend, with Rust acting as a coordinator that receives ready-made facts via `submit_asciidoc_facts`.

**ParserRegistry and asciidoc.rs stay unchanged.** The async delegation vs. sync fallback decision happens in `WorkspaceIndex::index_file()` via an `app_handle` check. When `app_handle` is `Some` (production), AsciiDoc parsing is delegated to the frontend. When `None` (tests), the existing synchronous Rust parser serves as a fallback, so all existing diagnostics/index tests continue to pass.

---

## Phase 1 — Domain Changes (Rust)

### 1.1 Add `DiagnosticKind::ParseError`

**File**: `src-tauri/src/domain/workspace_index.rs`

Add a variant to the `DiagnosticKind` enum:

```rust
pub enum DiagnosticKind {
    MissingInclude,
    MissingXrefDocument,
    MissingXrefAnchor,
    MissingImage,
    DuplicateAnchor,
    CircularInclude,
    ParseError,  // NEW
}
```

### 1.2 Define IPC contract types

Add to **`src-tauri/src/domain/workspace_index.rs`** (or a new `src-tauri/src/domain/asciidoc_facts.rs`):

```rust
#[derive(Serialize, Clone)]
pub struct AsciiDocParseRequested {
    pub document_id: DocumentId,
    pub version: u64,
    pub content: String,
    /// Relative path of the document. Included in the event payload for
    /// potential future use by the frontend (e.g., resolving include/image
    /// paths against the repo root). Not currently consumed by extractFacts.
    pub relative_path: PathBuf,
}

#[derive(Deserialize)]
pub struct AsciiDocFacts {
    pub anchors: Vec<AnchorFact>,
    pub includes: Vec<IncludeFact>,
    pub references: Vec<ReferenceFact>,
    pub attributes: Vec<AttributeFact>,
    pub images: Vec<ImageFact>,
    pub parse_errors: Vec<ParseErrorFact>,
}

#[derive(Deserialize)]
pub struct AnchorFact { pub id: String, pub line: u32, pub column: u32 }
#[derive(Deserialize)]
pub struct IncludeFact { pub path: String, pub line: u32, pub column: u32 }
#[derive(Deserialize)]
pub struct ReferenceFact { pub target_document: String, pub anchor: Option<String>, pub line: u32, pub column: u32 }
#[derive(Deserialize)]
pub struct AttributeFact { pub name: String, pub value: String, pub line: u32 }
#[derive(Deserialize)]
pub struct ImageFact { pub path: String, pub line: u32 }
#[derive(Deserialize)]
pub struct ParseErrorFact { pub message: String, pub line: Option<u32> }
```

Note: `relative_path` in `AsciiDocParseRequested` is included for future use (e.g., resolving relative `image::` paths in the frontend) but is not consumed in this implementation. It is intentionally kept — removing it would break forward-compatibility should a future feature (like inline image previews) need it, while keeping it costs nothing in payload size.

---

## Phase 2 — Index/Coordinator Changes (Rust)

### 2.1 Add coordinator fields to `WorkspaceIndex`

**File**: `src-tauri/src/services/workspace_index.rs`

```rust
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32};

// New fields in the struct.
// MAX_INFLIGHT is a field (not a const) so tests can inject a lower value.
doc_versions: DashMap<DocumentId, u64>,
pending_adoc_queue: RwLock<VecDeque<AsciiDocParseRequested>>,
parse_timeouts: DashMap<(DocumentId, u64), tokio::task::AbortHandle>,
inflight_adoc_count: AtomicU32,
max_inflight: u32,                  // production default = 8; tests can set to 1
build_adoc_pending: AtomicU32,      // tracks deferred adoc facts during initial build
building_in_progress: AtomicBool,   // true between IndexBuildingStarted and IndexBuildingFinished
frontend_ready: AtomicBool,
```

Initialize all to zero/false/default in `new()`. Add a `#[cfg(test)]` constructor that accepts `max_inflight`:

```rust
#[cfg(test)]
pub fn with_max_inflight(parsers: ParserRegistry, max_inflight: u32) -> Self {
    Self { max_inflight, ..Self::new(parsers) }
}
```

Clear all in `clear()`:
- `doc_versions.clear()`
- `*pending_adoc_queue.write().unwrap() = VecDeque::new()`
- `parse_timeouts.clear()`
- `inflight_adoc_count.store(0, Ordering::SeqCst)`
- `build_adoc_pending.store(0, Ordering::SeqCst)`
- `building_in_progress.store(false, Ordering::SeqCst)`
- `frontend_ready.store(false, Ordering::SeqCst)`

The `clear()` method already resets all state between builds; the coordinator queue must be included in that reset.

### 2.2 Branch `index_file()` for async delegation

**File**: `src-tauri/src/services/workspace_index.rs`

The existing method reads a file, calls `parsers.parse()`, then `insert_parsed()`. After inserting the `Document` record into `self.documents`, branch on doc type:

```rust
fn index_file(&self, self_arc: Arc<Self>, root: &Path, path: PathBuf, modified: SystemTime) -> Result<(), WorkspaceIndexError> {
    let content = std::fs::read_to_string(&path).map_err(|e| {
        WorkspaceIndexError::Message(format!("read {}: {}", path.display(), e))
    })?;
    let path_str = path.to_string_lossy().into_owned();

    let Some(doc_type) = self.parsers.doc_type(&path_str) else {
        return Ok(());
    };

    let relative = relative_key(root, &path)?;
    let id = DocumentId::new(relative.clone());
    let file_name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();

    let document = Document {
        id: id.clone(),
        absolute_path: path.to_string_lossy().into_owned(),
        relative_path: relative.clone(),
        file_name,
        doc_type,
        modified_at: unix_seconds(modified),
    };
    self.documents.insert(id.clone(), document);

    if doc_type == DocumentType::AsciiDoc && self.app_handle.read().unwrap().is_some() {
        // Production: delegate parsing to frontend asynchronously.
        self.dispatch_asciidoc_parse(self_arc, &id, content, relative);
    } else {
        // Tests or non-AsciiDoc: use synchronous parser.
        let parsed = self.parsers.parse(&path_str, &content);
        self.insert_parsed(&id, parsed);
    }

    Ok(())
}
```

Key: the async branch **only** runs when `app_handle` is set. In tests (`build()` without `set_app_handle`), the existing `parsers.parse()` path is taken — `ParserRegistry` and `ascii_doc.rs` remain unchanged.

**Arc plumbing**: `index_file` now takes `self_arc: Arc<Self>` in addition to `&self`. Callers pass `Arc::clone(&self)` down:

```rust
// In build(), inside the for-each-file loop:
self.index_file(Arc::clone(&index_arc), root, file.path.clone(), file.modified)?;

// In update_document() (which receives &self but the caller has Arc):
self.index_file(Arc::clone(&index_arc), &root, path, modified)?;
```

`WorkspaceIndex` is always behind `Arc` in Tauri state, so there's always an `Arc` at every call site.

### 2.3 Document version tracking

In `dispatch_asciidoc_parse(doc_id, ...)`:
- Use `doc_versions.entry(doc_id.clone()).and_modify(|v| *v += 1).or_insert(1)` — increment or start at 1.
- The resulting value is the version for this parse request.

On `remove_document(path)` (called on file deletion):
- **Remove** the entry from `doc_versions` — `doc_versions.remove(&id)`.
- This way, any stale `submit_asciidoc_facts` arriving for a deleted document will hit `doc_versions.get(doc_id)` returning `None`.

### 2.4 `dispatch_asciidoc_parse()` full logic

```rust
fn dispatch_asciidoc_parse(
    &self,
    self_arc: Arc<Self>,
    doc_id: &DocumentId,
    content: String,
    relative_path: String,
) {
    let version = *self.doc_versions
        .entry(doc_id.clone())
        .and_modify(|v| *v += 1)
        .or_insert(1);

    let payload = AsciiDocParseRequested {
        document_id: doc_id.clone(),
        version,
        content,
        relative_path: PathBuf::from(&relative_path),
    };

    // Spawn timeout task (see 2.7 for full implementation).
    let doc_id_for_timeout = doc_id.clone();
    let arc_for_timeout = Arc::clone(&self_arc);
    let handle = tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(PARSE_TIMEOUT_SECS)).await;
        arc_for_timeout.handle_parse_timeout(&doc_id_for_timeout, version);
    });
    self.parse_timeouts.insert((doc_id.clone(), version), handle.abort_handle());

    if !self.frontend_ready.load(std::sync::atomic::Ordering::SeqCst) {
        self.pending_adoc_queue.write().unwrap().push_back(payload);
        if self.building_in_progress.load(std::sync::atomic::Ordering::SeqCst) {
            self.build_adoc_pending.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        return;
    }

    let current = self.inflight_adoc_count.load(std::sync::atomic::Ordering::SeqCst);
    if current < self.max_inflight {
        self.inflight_adoc_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.try_emit_parse_request(payload);
    } else {
        self.pending_adoc_queue.write().unwrap().push_back(payload);
    }
    if self.building_in_progress.load(std::sync::atomic::Ordering::SeqCst) {
        self.build_adoc_pending.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

fn try_emit_parse_request(&self, payload: AsciiDocParseRequested) {
    if let Some(handle) = self.app_handle.read().unwrap().as_ref() {
        let _ = handle.emit("asciidoc:parse-requested", &payload);
    }
}
```

Note: `self.max_inflight` defaults to 8 in production; tests can inject 1 via `with_max_inflight(parsers, 1)` to verify queue behavior without needing 8 concurrent parses.

### 2.5 `submit_asciidoc_facts()` — unified finalize path (stale or not)

**Critical invariant**: every `dispatch_asciidoc_parse` that increments `inflight_adoc_count` and `build_adoc_pending` must have exactly one `submit_asciidoc_facts` call that decrements both — **even if the facts are stale**. Otherwise stale responses permanently leak the counter and eventually stop all dispatching. Likewise, `build_adoc_pending` would never reach zero, so `IndexBuildingFinished` would never be emitted.

The solution is a single "apply facts to index" conditional inside a shared bookkeeping tail, rather than an early return:

```rust
pub fn submit_asciidoc_facts(
    &self,
    doc_id: &DocumentId,
    version: u64,
    facts: AsciiDocFacts,
) -> Result<(), WorkspaceIndexError> {
    // --- Version check (do NOT return early — bookkeeping MUST always run) ---
    let is_valid = match self.doc_versions.get(doc_id) {
        Some(current) => *current == version, // version matches → valid
        None => false,                        // document was deleted → stale
    };

    // Only apply facts if the version is still current.
    if is_valid {
        let parsed = self.facts_to_parsed(doc_id, facts);
        self.remove_entries_for_doc(doc_id);
        self.insert_parsed(doc_id, parsed);
        diagnostics::run_for(self, doc_id);
        self.emit(IndexEvent::IndexUpdated { document: doc_id.0.clone() });
        self.emit(IndexEvent::DiagnosticsUpdated { document: doc_id.0.clone() });
    }
    // IMPORTANT: bookkeeping below runs unconditionally — both for valid
    // and stale responses.

    // --- Decrement inflight ---
    self.inflight_adoc_count.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);

    // --- Drain queue (lock released before body — next is owned) ---
    let next = self.pending_adoc_queue.write().unwrap().pop_front();
    if let Some(payload) = next {
        self.inflight_adoc_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.try_emit_parse_request(payload);
    }

    // --- Build completion check ---
    self.try_finish_build();

    Ok(())
}

fn try_finish_build(&self) {
    if !self.building_in_progress.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let pending = self.build_adoc_pending.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    if pending == 1 {
        // Last pending adoc fact arrived.
        diagnostics::run_all(self);
        let stats = self.compute_stats();
        self.building_in_progress.store(false, std::sync::atomic::Ordering::SeqCst);
        self.emit(IndexEvent::IndexBuildingFinished { stats });
    }
}
```

**Intentional trade-off — no queue deduplication**: if a file changes while its previous parse request is still in the queue (not yet dispatched), both versions enter the queue. The older one will eventually be dispatched, parsed by the frontend, and its `submit_asciidoc_facts` will hit `is_valid = false` because `doc_versions` has already been incremented past it. The stale facts are discarded by the conditional above, and bookkeeping proceeds normally. This wastes one frontend parse cycle but is safe and simple — suitable for MVP. If it becomes a measurable performance issue, deduplication by `document_id` can be added later (keep only the newest version for each doc_id in the queue).

**Helper: `facts_to_parsed`** — converts `AsciiDocFacts` into `ParsedDocument`, mapping each `AnchorFact` to an `Anchor` with `document: doc_id.clone()`, etc. Parse errors become `Diagnostic` entries with `DiagnosticKind::ParseError, Severity::Error`.

### 2.6 `frontend_ready()` — drain buffered queue

```rust
pub fn frontend_ready(&self) {
    self.frontend_ready.store(true, std::sync::atomic::Ordering::SeqCst);
    let mut queue = self.pending_adoc_queue.write().unwrap();
    loop {
        let current = self.inflight_adoc_count.load(std::sync::atomic::Ordering::SeqCst);
        if current >= self.max_inflight {
            break; // No more slots — the next submit_asciidoc_facts will drain further.
        }
        let next = queue.pop_front();
        drop(queue); // Release lock before dispatch
        if let Some(payload) = next {
            self.inflight_adoc_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.try_emit_parse_request(payload);
            queue = self.pending_adoc_queue.write().unwrap();
        } else {
            break; // Queue is empty — done draining.
        }
    }
}
```

Note: `drop(queue)` is explicit here because the `pop_front()` returns an owned value — it would drop naturally at the end of the loop body, but the explicit call makes the lock-release point unmistakably clear to future readers.

### 2.7 Timeout for hanging parse requests

If the frontend process hangs or crashes after receiving a parse request, `submit_asciidoc_facts` will never be called, and `inflight_adoc_count` will never decrement — eventually blocking all further parses when it reaches `MAX_INFLIGHT`.

The naive approach (spawned timeout task that calls `submit_asciidoc_facts` after N seconds) has a race: a valid response that arrives at 29.9 seconds will be processed successfully, but the timeout task at 30 seconds will see the same `version` (versions don't change on successful response) and call `submit_asciidoc_facts` a **second** time — double-decrementing `inflight_adoc_count` and potentially overwriting valid facts with a synthetic error.

**Solution**: `tokio::task::AbortHandle`. When a parse request is dispatched, spawn a timeout task and store its `AbortHandle`. When `submit_asciidoc_facts` is called (stale or valid), abort the timeout task **before** any bookkeeping. This guarantees exactly-once execution.

```rust
use tokio::task::JoinHandle;

// Add to struct fields:
parse_timeouts: DashMap<(DocumentId, u64), tokio::task::AbortHandle>,

const PARSE_TIMEOUT_SECS: u64 = 30;

fn dispatch_asciidoc_parse(
    &self,
    self_arc: Arc<Self>,
    doc_id: &DocumentId,
    content: String,
    relative_path: String,
) {
    // ... version increment and payload construction as before ...

    // Spawn timeout task.
    let doc_id_clone = doc_id.clone();
    let version_for_timeout = version;
    let arc_for_timeout = Arc::clone(&self_arc);
    let handle = tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(PARSE_TIMEOUT_SECS)).await;
        arc_for_timeout.handle_parse_timeout(&doc_id_clone, version_for_timeout);
    });
    self.parse_timeouts.insert((doc_id.clone(), version), handle.abort_handle());

    // ... queue/inflight logic as before ...
}

fn handle_parse_timeout(&self, doc_id: &DocumentId, version: u64) {
    // Remove our own timeout entry first — no other code path will touch it.
    // If a real response already arrived and called AbortHandle::abort(),
    // this remove returns None (the entry was already removed). That's fine.
    self.parse_timeouts.remove(&(doc_id.clone(), version));

    // Only fire if the version is still current (file wasn't re-modified).
    let is_current = match self.doc_versions.get(doc_id) {
        Some(v) => *v == version,
        None => false,
    };
    if !is_current {
        return;
    }
    // Synthesize a parse error and run through the standard path.
    let facts = AsciiDocFacts {
        anchors: vec![],
        includes: vec![],
        references: vec![],
        attributes: vec![],
        images: vec![],
        parse_errors: vec![ParseErrorFact {
            message: format!("parse timed out after {}s", PARSE_TIMEOUT_SECS),
            line: None,
        }],
    };
    let _ = self.submit_asciidoc_facts(doc_id, version, facts);
}
```

**In `submit_asciidoc_facts`, abort the timeout BEFORE any bookkeeping:**

```rust
pub fn submit_asciidoc_facts(
    &self,
    doc_id: &DocumentId,
    version: u64,
    facts: AsciiDocFacts,
) -> Result<(), WorkspaceIndexError> {
    // Abort the timeout for this specific (document, version) pair.
    // This guarantees exactly-once execution: if the response arrives
    // before the timeout fires, the timeout is cancelled. Conversely,
    // if the timeout fires first, its AbortHandle is already removed
    // and the real response (when it arrives) will find no handle to abort
    // but will still run through the bookkeeping path.
    if let Some((_, handle)) = self.parse_timeouts.remove(&(doc_id.clone(), version)) {
        handle.abort();
    }

    // ... rest of submit_asciidoc_facts (version check, apply, bookkeeping) ...
}
```

Note: `parse_timeouts` is keyed by `(DocumentId, u64)` — the document+version pair. When a response (valid, stale, or timeout) arrives for version N, only the `(doc_id, N)` entry is removed and aborted. A concurrent parse request for the same document at version N+1 has its own independent `(doc_id, N+1)` entry that is unaffected. This is critical because the plan explicitly allows multiple concurrent dispatches for the same document (see "no queue deduplication" trade-off in 2.5) — keying by `DocumentId` alone would cause a stale response at version N to abort the timeout for the still-in-flight version N+1, reintroducing the forever-hanging-build bug.

### 2.8 Build progress with deferred `IndexBuildingFinished`

In `build()`, after the scan loop finishes:

```rust
// After the for-each-file loop in build():
let adoc_pending = self.build_adoc_pending.load(std::sync::atomic::Ordering::SeqCst);
if adoc_pending == 0 {
    // No deferred AsciiDoc parses — emit immediately.
    diagnostics::run_all(self);
    let stats = self.compute_stats();
    self.emit(IndexEvent::IndexBuildingFinished { stats: stats.clone() });
} else {
    // AsciiDoc facts are still in flight. IndexBuildingFinished will be
    // emitted by try_finish_build() when the last fact arrives.
    self.building_in_progress.store(true, std::sync::atomic::Ordering::SeqCst);
    // Do NOT emit IndexBuildingFinished here — the status bar will show
    // "building" until all facts arrive.
}
```

The `building_in_progress` flag must also be set to `false` in `clear()`.

---

## Phase 3 — Tauri Commands & Events (Rust)

### 3.1 New file: `src-tauri/src/commands/asciidoc.rs`

```rust
use std::sync::Arc;
use tauri::State;
use crate::domain::workspace_index::{AsciiDocFacts, DocumentId};
use crate::services::workspace_index::WorkspaceIndex;

#[tauri::command]
pub fn submit_asciidoc_facts(
    index: State<'_, Arc<WorkspaceIndex>>,
    document_id: String,
    version: u64,
    facts: AsciiDocFacts,
) -> Result<(), String> {
    let doc_id = DocumentId::new(document_id);
    index.submit_asciidoc_facts(&doc_id, version, facts)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn frontend_ready(
    index: State<'_, Arc<WorkspaceIndex>>,
) -> Result<(), String> {
    index.frontend_ready();
    Ok(())
}
```

### 3.2 Register in module tree

**`src-tauri/src/commands/mod.rs`**: add `pub mod asciidoc;`

**`src-tauri/src/lib.rs`**: add to `generate_handler![]`:

```rust
commands::asciidoc::submit_asciidoc_facts,
commands::asciidoc::frontend_ready,
```

### 3.3 Capabilities

Tauri v2 auto-generates permission entries for custom `#[tauri::command]` functions. No changes to `default.json` are needed — the existing `"core:default"` permission covers custom commands.

### 3.4 Event emission channel

Rust emits on channel `"asciidoc:parse-requested"` with payload `AsciiDocParseRequested`. This is separate from the existing `"workspace-index://event"` channel to avoid conflating concerns.

---

## Phase 4 — Frontend (TypeScript)

### 4.1 Add `asciidoctor` dependency + verify API

```bash
bun add asciidoctor
```

**Before writing the hook**, verify the IncludeProcessor API against the `asciidoctor.js` documentation:

- The `process` method in Asciidoctor.js extensions receives `(doc, reader, target, attributes)`.
- The signature shown in the hook is a draft — consult the current docs via Context7 MCP for `@asciidoctor/core` to confirm the exact parameters and return type.
- The IncludeProcessor should record `{ path: target, line, column }` into a captured-array and return empty content (`""`) so `asciidoctor.load()` does not try to resolve the include path.

### 4.2 New lib wrapper: `src/lib/asciidocParser.ts`

```typescript
import { invoke } from "@tauri-apps/api/core";

export type AnchorFact = { id: string; line: number; column: number };
export type IncludeFact = { path: string; line: number; column: number };
export type ReferenceFact = { targetDocument: string; anchor: string | null; line: number; column: number };
export type AttributeFact = { name: string; value: string; line: number };
export type ImageFact = { path: string; line: number };
export type ParseErrorFact = { message: string; line: number | null };

export type AsciiDocFacts = {
  anchors: AnchorFact[];
  includes: IncludeFact[];
  references: ReferenceFact[];
  attributes: AttributeFact[];
  images: ImageFact[];
  parseErrors: ParseErrorFact[];
};

export function submitAsciiDocFacts(documentId: string, version: number, facts: AsciiDocFacts): Promise<void> {
  return invoke("submit_asciidoc_facts", { documentId, version, facts });
}

export function frontendReady(): Promise<void> {
  return invoke("frontend_ready");
}
```

### 4.3 New hook: `src/hooks/useAsciiDocParser.ts`

The critical difference from the original draft: **try/catch wraps the entire IPC round-trip**, not just `extractFacts`. If `submitAsciiDocFacts` itself throws (serialization error, IPC failure), the error is caught and a fallback `submitAsciiDocFacts` call is made with empty facts so the Rust coordinator is never left with a dangling inflight counter.

```typescript
import { listen } from "@tauri-apps/api/event";
import Asciidoctor from "asciidoctor";
import { useEffect } from "react";
import { type AsciiDocFacts, frontendReady, submitAsciiDocFacts } from "../lib/asciidocParser";

type AsciiDocParseRequested = {
  document_id: string;
  version: number;
  content: string;
  relative_path: string;
};

// Captured includes from the IncludeProcessor.
let capturedIncludes: { path: string; line: number; column: number }[] = [];

const asciidoctor = Asciidoctor();

asciidoctor.Extensions.register(function () {
  this.includeProcessor(function () {
    const self = this;
    self.handles(function (_target: string) {
      return true;
    });
    // IMPORTANT: signature verified against asciidoctor.js docs before implementation.
    self.process(function (_doc: any, reader: any, target: string, _attrs: Record<string, any>) {
      const lineNumber = reader != null ? reader.getLineNumber() ?? 0 : 0;
      capturedIncludes.push({
        path: target,
        line: (lineNumber as number) + 1,
        column: 1,
      });
      // Return empty content so the include is not expanded/resolved.
      return "";
    });
  });
});

function extractFacts(content: string): AsciiDocFacts {
  capturedIncludes = []; // Reset per-parse.

  const facts: AsciiDocFacts = {
    anchors: [],
    includes: [],
    references: [],
    attributes: [],
    images: [],
    parseErrors: [],
  };

  try {
    const doc = asciidoctor.load(content, {
      sourcemap: true,
      safe: "safe",
      attributes: { showtitle: true },
    });

    // 1. Anchors: walk AST for blocks with getSourceLocation().
    // 2. Includes: already captured by the IncludeProcessor above.
    // 3. Xrefs: walk AST for inline nodes with type "xref".
    // 4. Attributes: line-scan content (sourcemap doesn't cover them).
    // 5. Images: walk AST for block-level image nodes.
    //
    // Detailed AST walking logic is in Phase 5 below.
    //
    // Fallback: for any entity where sourcemap doesn't give line/column,
    // scan the raw `content` with regex to find the position.

    facts.includes = capturedIncludes;

  } catch (e) {
    facts.parseErrors.push({
      message: e instanceof Error ? e.message : String(e),
      line: null,
    });
  }

  return facts;
}

export function useAsciiDocParser() {
  useEffect(() => {
    let unlisten: (() => void) | null = null;

    listen<AsciiDocParseRequested>("asciidoc:parse-requested", async (event) => {
      const { document_id, version, content } = event.payload;

      let facts: AsciiDocFacts;
      try {
        facts = extractFacts(content);
      } catch (e) {
        facts = {
          anchors: [],
          includes: [],
          references: [],
          attributes: [],
          images: [],
          parseErrors: [{
            message: e instanceof Error ? e.message : String(e),
            line: null,
          }],
        };
      }

      // Always call submitAsciiDocFacts, even on error, so Rust can
      // decrement inflight_adoc_count and drain the queue.
      try {
        await submitAsciiDocFacts(document_id, version, facts);
      } catch (_submitError) {
        // If even the submit fails, attempt a second call with minimal
        // payload to unblock the coordinator.
        const emptyFacts: AsciiDocFacts = {
          anchors: [], includes: [], references: [],
          attributes: [], images: [],
          parseErrors: [{
            message: "IPC submit failed for this document",
            line: null,
          }],
        };
        try {
          await submitAsciiDocFacts(document_id, version, emptyFacts);
        } catch {
          // At this point we're truly stuck — this path should be
          // unreachable in practice. The Rust-side timeout
          // (PARSE_TIMEOUT_SECS) is the safety net.
        }
      }
    }).then((fn) => { unlisten = fn; });

    // Signal Rust that the frontend is ready to receive parse requests.
    // Buffered requests from before this point will be drained.
    frontendReady();

    return () => {
      if (unlisten) unlisten();
    };
  }, []);
}
```

### 4.4 Mount the hook in `App.tsx`

Add `useAsciiDocParser()` near the top of the component function, outside any conditionals (it must run unconditionally):

```typescript
function App() {
  useAsciiDocParser(); // Must be called unconditionally — registers the listener.

  const layout = useWorkspaceLayout();
  // ... rest of hooks ...
}
```

### 4.5 Update frontend `DiagnosticKind` type

**File**: `src/lib/workspaceIndex.ts`

Add `"parseError"` to the `DiagnosticKind` union:

```typescript
export type DiagnosticKind =
  | "missingInclude"
  | "missingXrefDocument"
  | "missingXrefAnchor"
  | "missingImage"
  | "duplicateAnchor"
  | "circularInclude"
  | "parseError";  // NEW
```

---

## Phase 5 — AST Walking Logic Details

The `extractFacts()` function:

1. **Anchors**: Walk AST blocks recursively. Block-level nodes with `getSourceLocation()` return `{ line, column }`. For inline anchors (`[#id]` not on a separate line), regex-scan the raw `content` with `\[#([^\]]+)\]` to get positions only — the anchor id itself comes from the AST node.

2. **Includes**: Handled by the `IncludeProcessor` extension registered once globally. It intercepts `include::` directives before resolution, records `{ path, line, column }` into a module-level array that is reset per call to `extractFacts`.

3. **Xrefs**: Walk AST for inline nodes (`node.getType() === "xref"` or similar). Extract `target` and `fragment` (anchor). If `sourcemap` is unavailable, line-scan content for `xref:([^\[\]]+)(?:#([^\[\]]+))?\[\]` to find positions.

4. **Attributes**: `sourcemap` does not cover `:name: value` lines. Line-scan the raw `content`: regex `^:(\w[\w-]*):\s*(.*)` applied per line. This is a position-only scan — no semantic interpretation — so it doesn't reintroduce the old parser's issues.

5. **Images**: Look for block-level image nodes in AST (type `"image"`). Fall back to line-scan for `image::([^\[\]]+)\[` if positions are unavailable.

6. **Full-parse failure**: If `asciidoctor.load()` throws, the `try/catch` in `extractFacts` captures it and returns empty facts + one `ParseErrorFact`. If the entire `extractFacts` call or `submitAsciiDocFacts` throws, the outer `try/catch` in the hook handles it (see 4.3).

---

## Phase 6 — Tests

### 6.1 Coordinator unit tests (Rust)

New file: **`src-tauri/src/services/tests_asciidoc_coordinator.rs`** (or inline `#[cfg(test)]` in `workspace_index.rs`).

Three test cases:

1. **Stale version discard**: Create a `WorkspaceIndex`, manually insert `doc_versions[doc_id] = 5`, call `submit_asciidoc_facts(doc_id, version=3, ...)`. Assert no facts are written to repositories.

2. **Deleted document discard**: Call `submit_asciidoc_facts(doc_id, ...)` for a `DocumentId` that was never inserted or was removed from `doc_versions`. Assert no panic and no facts written.

3. **Queue drain after `frontend_ready`**: Set `MAX_INFLIGHT` to 1 for the test. Dispatch 3 parses (2 go to queue), call `frontend_ready()`, verify first is emitted immediately and second is emitted when inflight decrements.

4. **Queue overflow**: Dispatch more than `MAX_INFLIGHT` parses, verify excess go to `pending_adoc_queue`.

### 6.2 `extractFacts` unit tests (TypeScript)

New file: **`src/__tests__/asciidocParser.test.ts`**

Test cases ported from the old Rust parser (`ascii_doc.rs` tests):

1. Block anchor: `[[installation]]\n= Installation\n` → 1 anchor, id=`"installation"`, line=1
2. Inline anchor: `[#configuration]\n` → 1 anchor, id=`"configuration"`
3. Include: `include::common.adoc[]\n` → 1 include, path=`"common.adoc"`
4. Xref with anchor: `xref:install.adoc#configuration[]\n` → 1 reference, targetDocument=`"install.adoc"`, anchor=`"configuration"`
5. Xref without anchor: `xref:install.adoc[]\n` → 1 reference, anchor=null
6. Attribute: `:product-name: DocFlow\n` → 1 attribute, name=`"product-name"`, value=`"DocFlow"`
7. Image: `image::images/auth.png[]\n` → 1 image, path=`"images/auth.png"`
8. Multiple constructs in one document
9. Parse error: feed syntactically invalid content, verify `parseErrors` is non-empty and other arrays are empty

Run with: `bun test` (or `bun run vitest` if Vitest is configured).

---

## Phase 7 — Cleanup & Verification

### 7.1 Nothing to remove

`ParserRegistry`, `ascii_doc.rs`, all imports — stay exactly as-is. The async delegation branch is in `index_file()`. No dead code to clean up.

### 7.2 Verify: existing tests still pass

The `app_handle.is_none()` check in `index_file()` ensures all existing `.adoc` tests take the synchronous parser path. Every test in `diagnostics.rs`, `workspace_index.rs`, and `ascii_doc.rs` must pass unchanged.

### 7.3 Run checks

```bash
cd src-tauri && cargo check
bun run tsc --noEmit
cd src-tauri && cargo test
bun test  # or bun run vitest run
```

---

## Files Changed Summary

| File | Action |
|------|--------|
| `src-tauri/src/domain/workspace_index.rs` | Add `DiagnosticKind::ParseError` and IPC contract types (`AsciiDocParseRequested`, `AsciiDocFacts`, fact structs) |
| `src-tauri/src/services/workspace_index.rs` | Add 7 coordinator fields (`doc_versions`, `pending_adoc_queue`, `inflight_adoc_count`, `build_adoc_pending`, `building_in_progress`, `frontend_ready`, `MAX_INFLIGHT`); branch `index_file()` for async delegation; implement `dispatch_asciidoc_parse()`, `submit_asciidoc_facts()`, `frontend_ready()`, `try_finish_build()`, timeout spawn; modify `build()` to defer `IndexBuildingFinished`; clear coordinator state in `clear()` |
| `src-tauri/src/commands/mod.rs` | Add `pub mod asciidoc;` |
| `src-tauri/src/commands/asciidoc.rs` | New: `submit_asciidoc_facts`, `frontend_ready` commands |
| `src-tauri/src/lib.rs` | Register 2 new commands in `generate_handler![]` |
| `src-tauri/capabilities/default.json` | No changes needed (Tauri v2 auto-covers custom commands via `core:default`) |
| `src-tauri/src/services/tests_asciidoc_coordinator.rs` | New: unit tests for stale version discard, deleted doc, queue drain, overflow |
| `package.json` | Add `asciidoctor` dependency |
| `src/lib/asciidocParser.ts` | New: typed IPC wrappers + fact types |
| `src/lib/workspaceIndex.ts` | Add `"parseError"` to `DiagnosticKind` union |
| `src/hooks/useAsciiDocParser.ts` | New: listens for `"asciidoc:parse-requested"`, runs `Asciidoctor.load()`, walks AST, submits facts with full error recovery |
| `src/App.tsx` | Add `useAsciiDocParser()` (unconditional, at hook call site) |
| `src/__tests__/asciidocParser.test.ts` | New: unit tests for `extractFacts` with sample adoc content |

**Files NOT changed:**
- `src-tauri/src/infra/parsers/ascii_doc.rs` — unchanged (test fallback)
- `src-tauri/src/infra/parsers/registry.rs` — unchanged (imports and dispatch logic stay)
- `src-tauri/src/services/diagnostics.rs` — unchanged
- `src-tauri/src/services/file_watcher.rs` — unchanged
