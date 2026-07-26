//! Unit tests for the AsciiDoc async coordinator logic in `WorkspaceIndex`.
//!
//! These tests verify:
//! - Stale version responses are discarded (facts not written).
//! - Responses for deleted documents are discarded.
//! - Queue overflow: dispatches beyond `max_inflight` are buffered.
//! - `frontend_ready` drains the buffered queue up to `max_inflight`.
//!
//! All tests use `with_max_inflight(1)` so queue behavior can be exercised
//! without dispatching many concurrent parses. The `app_handle` is `None`
//! in tests, so `try_emit_parse_request` is a no-op (events are not emitted),
//! but the queue/inflight/timeout bookkeeping still runs.

use std::sync::Arc;

use crate::domain::asciidoc_facts::{AsciiDocFacts, AnchorFact};
use crate::domain::workspace_index::{DocumentId, DocumentType};
use crate::infra::parsers::registry::ParserRegistry;
use crate::services::workspace_index::WorkspaceIndex;

fn empty_facts() -> AsciiDocFacts {
    AsciiDocFacts {
        anchors: vec![],
        includes: vec![],
        references: vec![],
        attributes: vec![],
        images: vec![],
        parse_errors: vec![],
    }
}

fn facts_with_anchor(id: &str) -> AsciiDocFacts {
    AsciiDocFacts {
        anchors: vec![AnchorFact {
            id: id.to_string(),
            line: 1,
            column: 1,
        }],
        includes: vec![],
        references: vec![],
        attributes: vec![],
        images: vec![],
        parse_errors: vec![],
    }
}

/// Insert a bare `Document` row (no parsing) so the index knows about the
/// document and `submit_asciidoc_facts` has somewhere to attach facts.
fn insert_doc(index: &WorkspaceIndex, id: &str) {
    use crate::domain::workspace_index::Document;
    let doc = Document {
        id: DocumentId::new(id.to_string()),
        absolute_path: format!("/tmp/{id}"),
        relative_path: id.to_string(),
        file_name: id.to_string(),
        doc_type: DocumentType::AsciiDoc,
        modified_at: 0,
    };
    index.documents.insert(DocumentId::new(id.to_string()), doc);
}

#[test]
fn submit_asciidoc_facts_discards_stale_version() {
    let index = Arc::new(WorkspaceIndex::with_max_inflight(
        ParserRegistry::new(),
        1,
    ));
    let doc_id = DocumentId::new("install.adoc");
    insert_doc(&index, "install.adoc");

    // Manually set the version to 5 (as if dispatch had been called 5 times).
    index.doc_versions.insert(doc_id.clone(), 5);

    // Submit facts for version 3 (stale). Should be discarded.
    index
        .submit_asciidoc_facts(&doc_id, 3, facts_with_anchor("stale"))
        .unwrap();

    // No anchors should have been written.
    assert!(
        index.find_anchor("stale").is_empty(),
        "stale facts must not be applied"
    );

    // Submit facts for version 5 (current). Should be applied.
    index
        .submit_asciidoc_facts(&doc_id, 5, facts_with_anchor("fresh"))
        .unwrap();
    assert_eq!(
        index.find_anchor("fresh").len(),
        1,
        "current-version facts must be applied"
    );
}

#[test]
fn submit_asciidoc_facts_discards_for_deleted_document() {
    let index = Arc::new(WorkspaceIndex::with_max_inflight(
        ParserRegistry::new(),
        1,
    ));
    let doc_id = DocumentId::new("gone.adoc");
    insert_doc(&index, "gone.adoc");

    // No entry in doc_versions — simulates a deleted document.
    index
        .submit_asciidoc_facts(&doc_id, 1, facts_with_anchor("ghost"))
        .unwrap();

    assert!(
        index.find_anchor("ghost").is_empty(),
        "facts for a deleted document must not be applied"
    );
}

#[test]
fn dispatch_buffers_beyond_max_inflight() {
    let index = Arc::new(WorkspaceIndex::with_max_inflight(
        ParserRegistry::new(),
        1,
    ));
    insert_doc(&index, "a.adoc");
    insert_doc(&index, "b.adoc");
    insert_doc(&index, "c.adoc");

    // Mark frontend ready first so dispatches go through the inflight path
    // (otherwise everything is buffered while frontend_ready is false).
    index.frontend_ready();

    // Dispatch 3 parses with max_inflight=1. The first should be in flight,
    // the other two buffered.
    index.dispatch_asciidoc_parse(
        &DocumentId::new("a.adoc"),
        "content a".to_string(),
        "a.adoc".to_string(),
    );
    index.dispatch_asciidoc_parse(
        &DocumentId::new("b.adoc"),
        "content b".to_string(),
        "b.adoc".to_string(),
    );
    index.dispatch_asciidoc_parse(
        &DocumentId::new("c.adoc"),
        "content c".to_string(),
        "c.adoc".to_string(),
    );

    assert_eq!(
        index.inflight_adoc_count.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "only one parse should be in flight"
    );
    assert_eq!(
        index.pending_adoc_queue.read().unwrap().len(),
        2,
        "two parses should be buffered"
    );
}

#[test]
fn frontend_ready_drains_queue() {
    let index = Arc::new(WorkspaceIndex::with_max_inflight(
        ParserRegistry::new(),
        1,
    ));
    insert_doc(&index, "a.adoc");
    insert_doc(&index, "b.adoc");

    // Dispatch 2 parses while frontend is NOT ready. Both should be buffered.
    index.dispatch_asciidoc_parse(
        &DocumentId::new("a.adoc"),
        "content a".to_string(),
        "a.adoc".to_string(),
    );
    index.dispatch_asciidoc_parse(
        &DocumentId::new("b.adoc"),
        "content b".to_string(),
        "b.adoc".to_string(),
    );
    assert_eq!(
        index.pending_adoc_queue.read().unwrap().len(),
        2,
        "both parses should be buffered before frontend_ready"
    );
    assert_eq!(
        index.inflight_adoc_count.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "no parses should be in flight before frontend_ready"
    );

    // Mark frontend ready — one parse should be dispatched (max_inflight=1),
    // the other remains buffered.
    index.frontend_ready();
    assert_eq!(
        index.inflight_adoc_count.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "one parse should be in flight after frontend_ready"
    );
    assert_eq!(
        index.pending_adoc_queue.read().unwrap().len(),
        1,
        "one parse should remain buffered"
    );

    // Submitting facts for the in-flight parse should drain one more from
    // the queue.
    let in_flight_doc = DocumentId::new("a.adoc");
    let version = *index.doc_versions.get(&in_flight_doc).unwrap();
    index
        .submit_asciidoc_facts(&in_flight_doc, version, empty_facts())
        .unwrap();
    assert_eq!(
        index.inflight_adoc_count.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "queue should have drained one more parse"
    );
    assert!(
        index.pending_adoc_queue.read().unwrap().is_empty(),
        "queue should be empty after drain"
    );
}

#[test]
fn submit_always_decrements_inflight_even_when_stale() {
    // Critical invariant: a stale response must still decrement
    // `inflight_adoc_count` so the queue can drain. Without this, a stale
    // response would permanently leak the counter and stall the pipeline.
    let index = Arc::new(WorkspaceIndex::with_max_inflight(
        ParserRegistry::new(),
        1,
    ));
    insert_doc(&index, "a.adoc");
    insert_doc(&index, "b.adoc");

    // Mark frontend ready so dispatches go through the inflight path.
    index.frontend_ready();

    // Dispatch a.adoc (in flight) and b.adoc (buffered, max_inflight=1).
    index.dispatch_asciidoc_parse(
        &DocumentId::new("a.adoc"),
        "content a".to_string(),
        "a.adoc".to_string(),
    );
    index.dispatch_asciidoc_parse(
        &DocumentId::new("b.adoc"),
        "content b".to_string(),
        "b.adoc".to_string(),
    );
    assert_eq!(
        index.inflight_adoc_count.load(std::sync::atomic::Ordering::SeqCst),
        1
    );

    // Bump a.adoc's version so the original dispatch is now stale.
    *index.doc_versions.get_mut(&DocumentId::new("a.adoc")).unwrap() = 99;

    // Submit stale facts for a.adoc version 1.
    index
        .submit_asciidoc_facts(&DocumentId::new("a.adoc"), 1, empty_facts())
        .unwrap();

    // The stale response must still have drained the queue: b.adoc should
    // now be in flight.
    assert_eq!(
        index.inflight_adoc_count.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "stale response must decrement inflight and drain queue"
    );
    assert!(
        index.pending_adoc_queue.read().unwrap().is_empty(),
        "queue must be drained after stale response"
    );
}
