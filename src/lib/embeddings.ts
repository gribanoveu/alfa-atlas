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
};

export type ModelDownloadProgress = {
  progress: number;
  error?: string;
  cancelled?: boolean;
};

// Mirrors the Rust `SyncPhase`/`SyncProgressPayload` emitted by
// `commands::embeddings::embedding_sync`.
export type SyncPhase = "chunking" | "embedding";

export type SyncProgress = {
  phase: SyncPhase;
  current: number;
  total: number;
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
