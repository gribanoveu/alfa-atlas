import { invoke } from "@tauri-apps/api/core";

export type AnchorFact = { id: string; line: number; column: number };
export type IncludeFact = { path: string; line: number; column: number };
export type ReferenceFact = {
  targetDocument: string;
  anchor: string | null;
  line: number;
  column: number;
};
export type AttributeFact = { name: string; value: string; line: number };
export type ImageFact = { path: string; line: number };
export type ParseErrorFact = {
  message: string;
  line: number | null;
  /** Severity label from asciidoctor's `LogMessage.getSeverity()` (lowercased). */
  severity: string;
};

export type AsciiDocFacts = {
  anchors: AnchorFact[];
  includes: IncludeFact[];
  references: ReferenceFact[];
  attributes: AttributeFact[];
  images: ImageFact[];
  parseErrors: ParseErrorFact[];
};

/**
 * Submit parsed AsciiDoc facts back to the Rust coordinator.
 *
 * Always called — even on parse failure — so the coordinator can decrement
 * `inflight_adoc_count` and drain the queue. The Rust side matches the
 * `documentId` / `version` against its `doc_versions` map and discards
 * stale responses.
 */
export function submitAsciiDocFacts(
  documentId: string,
  version: number,
  facts: AsciiDocFacts,
): Promise<void> {
  return invoke("submit_asciidoc_facts", { documentId, version, facts });
}

/**
 * Signal the Rust coordinator that the frontend listener for
 * `asciidoc:parse-requested` is mounted. Buffered requests will be drained
 * up to `max_inflight`.
 */
export function frontendReady(): Promise<void> {
  return invoke("frontend_ready");
}
