import { invoke } from "@tauri-apps/api/core";

export type DocumentType =
  | "asciiDoc"
  | "markdown"
  | "json"
  | "yaml"
  | "text"
  | "plantUml"
  | "mermaid";

export type Severity = "error" | "warning";

export type DiagnosticKind =
  | "missingInclude"
  | "missingXrefDocument"
  | "missingXrefAnchor"
  | "missingImage"
  | "duplicateAnchor"
  | "circularInclude"
  | "parseError"
  /** Атрибуты шапки отделены от заголовка пустой строкой — `:toc:` не действует. */
  | "detachedHeaderAttributes";

export type Document = {
  id: string;
  absolutePath: string;
  relativePath: string;
  fileName: string;
  docType: DocumentType;
  modifiedAt: number;
};

export type Anchor = {
  id: string;
  document: string;
  line: number;
  column: number;
};

export type Include = {
  path: string;
  sourceDocument: string;
  line: number;
  column: number;
};

export type Reference = {
  targetDocument: string;
  anchor: string | null;
  sourceDocument: string;
  line: number;
  column: number;
};

export type Attribute = {
  name: string;
  value: string;
  document: string;
  line: number;
};

export type Image = {
  path: string;
  document: string;
  line: number;
};

export type Diagnostic = {
  kind: DiagnosticKind;
  message: string;
  document: string;
  line: number;
  column: number;
  severity: Severity;
};

export type IndexStats = {
  documents: number;
  anchors: number;
  includes: number;
  references: number;
  attributes: number;
  images: number;
  warnings: number;
  errors: number;
};

export type IndexEvent =
  | { kind: "indexBuildingStarted" }
  | {
      kind: "indexBuildingProgress";
      payload: { done: number; total: number; current: string };
    }
  | { kind: "indexBuildingFinished"; payload: { stats: IndexStats } }
  | { kind: "indexUpdated"; payload: { document: string } }
  | { kind: "diagnosticsUpdated"; payload: { document: string } };

export const INDEX_EVENT_CHANNEL = "workspace-index://event";

export function buildIndex(repoRoot: string): Promise<IndexStats> {
  return invoke<IndexStats>("build_index", { repoRoot });
}

export function clearIndex(): Promise<void> {
  return invoke<void>("clear_index");
}

export function indexIsOpen(): Promise<boolean> {
  return invoke<boolean>("index_is_open");
}

export function getDocument(path: string): Promise<Document | null> {
  return invoke<Document | null>("get_document", { path });
}

/** Same lookup as `getDocument`, but by repo-relative `DocumentId` key
 * rather than a filesystem path — the backend's `get_document` resolves
 * `path` against the real filesystem (canonicalize), so it needs an
 * absolute path; this variant looks up the index's own string key
 * directly, which is all the frontend ever has on hand for a document it
 * hasn't opened yet (e.g. resolving an `include::`/`xref:` target). */
export function getDocumentById(id: string): Promise<Document | null> {
  return invoke<Document | null>("get_document_by_id", { id });
}

export function getDocuments(): Promise<Document[]> {
  return invoke<Document[]>("get_documents");
}

export function findDocument(name: string): Promise<Document[]> {
  return invoke<Document[]>("find_document", { name });
}

export function findAnchor(id: string): Promise<Anchor[]> {
  return invoke<Anchor[]>("find_anchor", { id });
}

export function findAnchors(documentId: string): Promise<Anchor[]> {
  return invoke<Anchor[]>("find_anchors", { documentId });
}

export function findIncludes(documentId: string): Promise<Include[]> {
  return invoke<Include[]>("find_includes", { documentId });
}

export function findReferences(documentId: string): Promise<Reference[]> {
  return invoke<Reference[]>("find_references", { documentId });
}

export function findAttribute(name: string): Promise<Attribute[]> {
  return invoke<Attribute[]>("find_attribute", { name });
}

export function getAttributes(documentId: string): Promise<Attribute[]> {
  return invoke<Attribute[]>("get_attributes", { documentId });
}

export function findImage(path: string): Promise<Image[]> {
  return invoke<Image[]>("find_image", { path });
}

export function getDiagnostics(): Promise<Diagnostic[]> {
  return invoke<Diagnostic[]>("get_diagnostics");
}

export function getDiagnosticsFor(documentId: string): Promise<Diagnostic[]> {
  return invoke<Diagnostic[]>("get_diagnostics_for", { documentId });
}
