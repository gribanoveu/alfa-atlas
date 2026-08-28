import { invoke } from "@tauri-apps/api/core";

// Mirrors domain::artifact and domain::artifact_render in
// src-tauri/src/domain/. Kept in sync by hand — there is no codegen here.

/** Mirrors `domain::artifact::ArtifactKind`. */
export type ArtifactKind = "httpRequest";

/** Mirrors `domain::artifact::ArtifactStatus`. */
export type ArtifactStatus = "draft" | "ready";

/** Russian label per kind — the Rust side deliberately carries no UI copy. */
export const ARTIFACT_KIND_LABELS: Record<ArtifactKind, string> = {
  httpRequest: "HTTP-запрос",
};

/** Mirrors `domain::artifact::ParamSpec` — one row of a parameter table,
 *  five fields for the template's five columns. */
export type ParamSpec = {
  name: string;
  format: string;
  required: boolean;
  description: string;
  values: string;
};

/** Mirrors `domain::artifact::BodySpec`. */
export type BodySpec = {
  mediaType: string;
  sample: string;
  params: ParamSpec[];
};

/** Mirrors `domain::artifact::ResponseSpec`. */
export type ResponseSpec = {
  status: string;
  description: string;
  sample: string;
  params: ParamSpec[];
};

/** Mirrors `domain::artifact::ErrorSpec`. */
export type ErrorSpec = {
  code: string;
  description: string;
};

/** Mirrors `domain::artifact::HttpRequestSpec`. */
export type HttpRequestSpec = {
  method: string;
  baseUrl: string;
  path: string;
  pathParams: ParamSpec[];
  queryParams: ParamSpec[];
  headers: ParamSpec[];
  body: BodySpec | null;
  responses: ResponseSpec[];
  errors: ErrorSpec[];
  notes: string | null;
};

/** Mirrors `domain::artifact::ArtifactContent` (internally tagged). */
export type ArtifactContent = { kind: "httpRequest" } & HttpRequestSpec;

/** Mirrors `domain::artifact::ArtifactRecord`. */
export type ArtifactRecord = {
  id: string;
  kind: ArtifactKind;
  title: string;
  purpose: string | null;
  status: ArtifactStatus;
  content: ArtifactContent;
  createdAtMs: number;
  updatedAtMs: number;
  chatId: string | null;
  repoRoot: string | null;
};

/** Mirrors `domain::artifact::ArtifactSummary`. */
export type ArtifactSummary = {
  id: string;
  kind: ArtifactKind;
  title: string;
  status: ArtifactStatus;
  subtitle: string;
  createdAtMs: number;
  updatedAtMs: number;
};

/** Mirrors `domain::artifact_render::RenderedHttpRequest`. */
export type RenderedHttpRequest = {
  inputParams: string;
  outputParams: string;
  curl: string;
  responseExamples: string;
  errors: string;
  requestAdoc: string;
  responseAdoc: string;
};

/** Mirrors `domain::artifact_render::RenderedArtifact`. */
export type RenderedArtifact = { kind: "httpRequest" } & RenderedHttpRequest;

export function artifactList(): Promise<ArtifactSummary[]> {
  return invoke<ArtifactSummary[]>("artifact_list");
}

export function artifactGet(artifactId: string): Promise<ArtifactRecord> {
  return invoke<ArtifactRecord>("artifact_get", { artifactId });
}

export function artifactCreateDraft(args: {
  kind: ArtifactKind;
  title: string;
  purpose?: string | null;
  prefill?: ArtifactContent | null;
  chatId?: string | null;
}): Promise<ArtifactRecord> {
  return invoke<ArtifactRecord>("artifact_create_draft", {
    kind: args.kind,
    title: args.title,
    purpose: args.purpose ?? null,
    prefill: args.prefill ?? null,
    chatId: args.chatId ?? null,
  });
}

/** Only `title`, `status` and `content` are honoured — the backend keeps
 *  provenance (`createdAtMs`/`chatId`/`purpose`) from the stored record. */
export function artifactSave(record: ArtifactRecord): Promise<ArtifactRecord> {
  return invoke<ArtifactRecord>("artifact_save", { record });
}

export function artifactDelete(artifactId: string): Promise<void> {
  return invoke<void>("artifact_delete", { artifactId });
}

/** Pure projection — the same renderer the assistant's tool result goes
 *  through, so the builder's preview cannot drift from what gets sent. */
export function artifactRender(content: ArtifactContent): Promise<RenderedArtifact> {
  return invoke<RenderedArtifact>("artifact_render", { content });
}

/** Empty content for a fresh draft, mirroring
 *  `ArtifactContent::empty_for`/`HttpRequestSpec::default()` — used when
 *  creating one locally before a round-trip. `baseUrl` mirrors the Rust
 *  default: the house `https://{host}/...` endpoint convention
 *  (`method-spec` skill, `references/structure.md`), not empty, so a fresh
 *  draft already has a valid endpoint token. */
export function emptyArtifactContent(kind: ArtifactKind): ArtifactContent {
  switch (kind) {
    case "httpRequest":
      return {
        kind: "httpRequest",
        method: "GET",
        baseUrl: "https://{host}/",
        path: "",
        pathParams: [],
        queryParams: [],
        headers: [],
        body: null,
        responses: [],
        errors: [],
        notes: null,
      };
  }
}
