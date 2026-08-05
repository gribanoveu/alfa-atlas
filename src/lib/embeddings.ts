import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type EmbeddingProviderKind = "local" | "remote";

// Mirrors `domain::embeddings::EmbeddingProviderConfig` in
// `src-tauri/src/domain/embeddings.rs`.
export type EmbeddingProviderConfig = {
  kind: EmbeddingProviderKind;
  remoteBaseUrl: string | null;
  remoteModel: string | null;
  remoteDimensions: number | null;
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
  /** Files the repo walk found but that haven't been chunked yet — always
   * `0` outside a fresh project's first-sync backlog (see `SyncTrigger`'s
   * `"background"` value). Non-zero means the rest of the repo is still
   * being indexed in the background; the index itself is safe to use in
   * the meantime, just incomplete. */
  backgroundPending: number;
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

export function getEmbeddingConfig(): Promise<EmbeddingProviderConfig> {
  return invoke<EmbeddingProviderConfig>("embedding_get_config");
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
