import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type EmbeddingProviderKind = "local" | "remote";

// Mirrors `domain::embeddings::ResolvedEmbeddingConfig` — what
// `embedding_get_config` returns (bundled preset merged with overrides).
export type ResolvedEmbeddingConfig = {
  kind: EmbeddingProviderKind;
  remoteBaseUrl: string | null;
  remoteModel: string | null;
  remoteDimensions: number | null;
  remoteTrustedCertPem: string | null;
  remoteSystemId: string | null;
  remoteDisableTlsVerification: boolean;
};

// Mirrors `domain::embeddings::EmbeddingProviderConfig` — the settings-layer
// override persisted by `embedding_set_config`. `kind: null` means inherit
// from the bundled preset.
export type EmbeddingProviderConfig = {
  kind: EmbeddingProviderKind | null;
  remoteBaseUrl: string | null;
  remoteModel: string | null;
  remoteDimensions: number | null;
  remoteTrustedCertPem: string | null;
  remoteSystemId: string | null;
  remoteDisableTlsVerification: boolean | null;
};

// Mirrors `domain::embeddings::ModelStatus` (adjacently tagged,
// `#[serde(tag = "status")]`).
export type ModelStatus =
  | { status: "notDownloaded" }
  | { status: "downloading"; progress: number }
  | { status: "ready" }
  | { status: "error"; message: string };

export type SyncStats = {
  embedded: number;
  skippedUnchanged: number;
  removed: number;
};

// Mirrors `domain::embeddings::EmbeddingIndexStatus`.
export type EmbeddingIndexStatus = {
  synced: boolean;
  embeddedCount: number;
  /** Persisted index exists but predates a version bump (or a different
   * index root) — left untouched on disk, not loaded; needs a real sync to
   * repair, distinct from "never synced". */
  stale: boolean;
  /** Files still queued for background processing (see `SyncTrigger`'s
   * `"background"` value) — `0` when nothing's deferred, which is most
   * syncs (a small non-doc change set is folded into the synchronous pass
   * instead). Non-zero after a fresh project's first sync, or after a
   * routine sync catches a large upstream change; documentation itself is
   * always prioritized ahead of this backlog, on every sync. The index
   * itself is safe to use in the meantime, just incomplete outside
   * `docs_root`. */
  backgroundPending: number;
};

// Mirrors `commands::repo_index::RepoIndexSummary` — a live snapshot of
// `RepositoryIndex`/`ChunkIndex`'s current resident state (no walk, no I/O,
// safe to call any time; reports whatever the last sync left resident, or
// all-zero before the first one).
export type RepoIndexSummary = {
  filesIndexed: number;
  byLanguage: Record<string, number>;
  chunksIndexed: number;
};

export type ModelDownloadProgress = {
  progress: number;
  error?: string;
  cancelled?: boolean;
};

// Mirrors the Rust `SyncPhase`/`SyncProgressPayload` emitted by
// `commands::embeddings::embedding_sync`.
export type SyncPhase = "chunking" | "embedding";

// Mirrors the Rust `SyncTrigger` — distinguishes a full, user-triggered
// `embedding_sync` from a file-watcher-driven incremental per-file tick,
// and from the low-priority backlog catch-up that follows a fresh
// project's first sync.
export type SyncTrigger = "full" | "incremental" | "background";

export type SyncProgress = {
  phase: SyncPhase;
  current: number;
  total: number;
  trigger: SyncTrigger;
};

export function getEmbeddingConfig(): Promise<ResolvedEmbeddingConfig> {
  return invoke<ResolvedEmbeddingConfig>("embedding_get_config");
}

export function setEmbeddingConfig(config: EmbeddingProviderConfig): Promise<void> {
  return invoke("embedding_set_config", { config });
}

/** Write-only — there is no `getEmbeddingRemoteApiKey`, only a status check. */
export function setEmbeddingRemoteApiKey(apiKey: string): Promise<void> {
  return invoke("embedding_set_remote_api_key", { apiKey });
}

export function hasEmbeddingRemoteApiKey(): Promise<boolean> {
  return invoke<boolean>("embedding_has_remote_api_key");
}

export function getEmbeddingModelStatus(): Promise<ModelStatus> {
  return invoke<ModelStatus>("embedding_model_status");
}

/** Resolves once the (potentially multi-minute) download finishes or fails
 * — subscribe via `listenModelDownloadProgress` for live status meanwhile. */
export function downloadEmbeddingModel(): Promise<void> {
  return invoke("embedding_download_model");
}

/** The underlying blocking download has no interrupt hook, so this can't
 * stop in-flight network I/O — it only tells the backend (and the UI) to
 * stop trusting whatever that attempt eventually reports back. */
export function cancelEmbeddingModelDownload(): Promise<void> {
  return invoke("embedding_cancel_model_download");
}

/** Rebuilds the repo/chunk index for the current project and reconciles
 * embeddings against it (new chunk → embed, changed hash → re-embed,
 * deleted chunk → drop its vector). */
export function syncEmbeddings(): Promise<SyncStats> {
  return invoke<SyncStats>("embedding_sync");
}

/** Read-only: is the current project's index already built? Backed by the
 * persisted/resident `EmbeddingIndex` itself, not by whether `syncEmbeddings`
 * happened to run earlier in this session — safe to call on every mount to
 * recover real state after a remount. */
export function getEmbeddingIndexStatus(): Promise<EmbeddingIndexStatus> {
  return invoke<EmbeddingIndexStatus>("embedding_index_status");
}

/** Read-only, cheap (in-memory only): the current project's `RepositoryIndex`/
 * `ChunkIndex` state as of the last sync — per-language file counts and a
 * chunk count, previously computed on every full sync and silently
 * discarded (`services::repo_index::RepoIndexStats`), now exposed directly. */
export function getRepoIndexSummary(): Promise<RepoIndexSummary> {
  return invoke<RepoIndexSummary>("repo_index_summary");
}

export function listenModelDownloadProgress(
  onProgress: (payload: ModelDownloadProgress) => void,
): Promise<UnlistenFn> {
  return listen<ModelDownloadProgress>("embedding:model-download-progress", (event) =>
    onProgress(event.payload),
  );
}

/** Fires repeatedly while a `syncEmbeddings()` call is in flight — first
 * `phase: "chunking"` (re-chunking changed files), then `phase: "embedding"`
 * (calling the provider in batches). Not emitted at all for a no-op sync
 * (nothing changed since the last one). */
export function listenSyncProgress(
  onProgress: (payload: SyncProgress) => void,
): Promise<UnlistenFn> {
  return listen<SyncProgress>("embedding:sync-progress", (event) => onProgress(event.payload));
}

/** Stops the file-watcher-driven incremental sync, if one is running for
 * the current project. Call when a project closes without a new one
 * opening in the same session — otherwise the backend's own attach logic
 * naturally swaps the watcher to whichever project opens next. */
export function teardownIncrementalWatcher(): Promise<void> {
  return invoke("embedding_index_teardown");
}

/** Tells the backend which files are open in the editor, for `syncEmbeddings`
 * to prioritize on a fresh project's first sync (open files + their direct
 * includes/xrefs get chunked+embedded before the rest of the repo). Pass
 * `EditorTab.path` values verbatim — already relative to the project's docs
 * root, exactly what the backend expects; no conversion needed here. */
export function setEmbeddingPriorityFiles(relativePaths: string[]): Promise<void> {
  return invoke("embedding_set_priority_files", { relativePaths });
}
