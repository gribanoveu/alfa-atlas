import { useCallback, useEffect, useRef, useState } from "react";
import {
  cancelEmbeddingModelDownload,
  downloadEmbeddingModel,
  getEmbeddingConfig,
  getEmbeddingIndexStatus,
  getEmbeddingModelStatus,
  getRepoIndexSummary,
  hasEmbeddingRemoteApiKey,
  listenModelDownloadProgress,
  listenSyncProgress,
  setEmbeddingConfig,
  setEmbeddingRemoteApiKey,
  syncEmbeddings,
  type EmbeddingIndexStatus,
  type EmbeddingProviderConfig,
  type ModelStatus,
  type RepoIndexSummary,
  type ResolvedEmbeddingConfig,
  type SyncProgress,
  type SyncStats,
} from "../lib/embeddings";

/**
 * Embedding provider config + local-model readiness + last sync result, in
 * one place — both `EmbeddingsTab` (Settings) and `AssistantPanel`
 * (RightDock checklist) call this so they read the same backend state via
 * the same logic, even though each holds its own React state instance
 * (the two are never visible at once — Settings is a modal overlay — so
 * there's no simultaneous-divergence case to guard against here).
 *
 * `lastSync` is this-session-only (the delta from the last `sync()` call);
 * `indexStatus` is fetched fresh on every `refresh()` (including the
 * mount-time one) from the backend's persisted/resident index, so "is the
 * index already built" survives this hook remounting — e.g. `AssistantPanel`
 * unmounting when the RightDock panel is hidden or another tool tab is
 * selected — instead of resetting every time.
 *
 * `repoRoot` keys per-project UI state: on switch/close, progress and
 * index status from the previous project are cleared so the StatusBar
 * never keeps showing the old project's "synced"/"syncing" after a mid-sync
 * close. A generation counter ignores late progress events and late
 * `sync()`/`refresh()` settlements that still belong to the previous root.
 */
export function useEmbeddingSetup(repoRoot: string | null = null) {
  const [config, setConfigState] = useState<ResolvedEmbeddingConfig | null>(null);
  const [modelStatus, setModelStatus] = useState<ModelStatus>({ status: "notDownloaded" });
  const [hasApiKey, setHasApiKey] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastSync, setLastSync] = useState<SyncStats | null>(null);
  const [indexStatus, setIndexStatus] = useState<EmbeddingIndexStatus | null>(null);
  const [repoIndexSummary, setRepoIndexSummary] = useState<RepoIndexSummary | null>(null);
  const [syncProgress, setSyncProgress] = useState<SyncProgress | null>(null);
  // The low-priority backlog catch-up after a fresh project's first sync —
  // deliberately separate from `syncProgress`/`busy` (see the listener
  // below) so it never makes the manual "Синхронизировать" action look
  // busy while it runs.
  const [backgroundSyncProgress, setBackgroundSyncProgress] = useState<SyncProgress | null>(null);
  // The backend can't truly abort an in-flight download (see
  // `cancelEmbeddingModelDownload`'s doc comment) — this just tells
  // `downloadModel`'s catch block that a rejection is an expected
  // cancellation, not a real failure to surface as an error.
  const cancelRequestedRef = useRef(false);
  // Bumped on every `repoRoot` change so in-flight IPC / progress from the
  // previous project can't write into the new project's UI state.
  const generationRef = useRef(0);
  const repoRootRef = useRef(repoRoot);
  repoRootRef.current = repoRoot;

  const refresh = useCallback(async () => {
    const generation = generationRef.current;
    const root = repoRootRef.current;
    try {
      const [nextConfig, nextStatus, nextHasKey, nextIndexStatus, nextRepoIndexSummary] =
        await Promise.all([
          getEmbeddingConfig(),
          getEmbeddingModelStatus(),
          hasEmbeddingRemoteApiKey(),
          root ? getEmbeddingIndexStatus() : Promise.resolve(null),
          root ? getRepoIndexSummary() : Promise.resolve(null),
        ]);
      if (generation !== generationRef.current || repoRootRef.current !== root) {
        return;
      }
      setConfigState(nextConfig);
      setModelStatus(nextStatus);
      setHasApiKey(nextHasKey);
      setIndexStatus(nextIndexStatus);
      setRepoIndexSummary(nextRepoIndexSummary);
      setError(null);
    } catch (e) {
      if (generation !== generationRef.current || repoRootRef.current !== root) {
        return;
      }
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    generationRef.current += 1;
    setSyncProgress(null);
    setBackgroundSyncProgress(null);
    setIndexStatus(null);
    setRepoIndexSummary(null);
    setLastSync(null);
    setError(null);
    setBusy(false);
    void refresh();
  }, [repoRoot, refresh]);

  // Live download progress, independent of `refresh` — the backend emits
  // this while `downloadEmbeddingModel()`'s promise is still in flight.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listenModelDownloadProgress((payload) => {
      if (payload.cancelled) {
        setModelStatus({ status: "notDownloaded" });
        return;
      }
      if (payload.error) {
        setModelStatus({ status: "error", message: payload.error });
        return;
      }
      setModelStatus(
        payload.progress >= 1
          ? { status: "ready" }
          : { status: "downloading", progress: payload.progress },
      );
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Live sync progress — the backend emits this while `syncEmbeddings()`'s
  // promise is still in flight, and separately while a fresh project's
  // background backlog task is catching up on the rest of the repo.
  // `full`-triggered payloads drive the manual-sync progress display;
  // `incremental` ticks (file-watcher-driven) are ignored here (`busy` can
  // briefly be `true` while a manual sync waits on the backend's sync
  // guard to let an in-flight incremental tick finish — an unfiltered
  // incremental payload arriving in that window would show the wrong
  // current/total numbers); `background` ticks update a separate piece of
  // state instead of `syncProgress`/`busy`, so the backlog catch-up never
  // makes the "Синхронизировать" action look busy. A `background`
  // chunking-phase tick (fired once per backlog batch, not per file) also
  // opportunistically refreshes `indexStatus` so `backgroundPending` stays
  // reasonably current without polling.
  //
  // Progress is ignored while no project is open (`repoRoot` null) so a
  // dying sync from a just-closed project can't resurrect "syncing" UI.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listenSyncProgress((payload) => {
      if (!repoRootRef.current) return;
      const generation = generationRef.current;
      if (payload.trigger === "full") {
        setSyncProgress(payload);
        // A passive instance of this hook (one that never calls `sync()`
        // itself — e.g. the status bar, which only ever observes) has no
        // other signal that a sync triggered elsewhere just finished, so it
        // would otherwise show a stale `indexStatus`/frozen progress
        // forever. Mirrors what the initiating instance's own `sync()`
        // already does in its `finally` block once the embedding phase
        // reaches 100% — harmless to also run there (an idempotent extra
        // refresh/reset).
        if (payload.phase === "embedding" && payload.current >= payload.total) {
          setSyncProgress(null);
          if (generation === generationRef.current) {
            void refresh();
          }
        }
      } else if (payload.trigger === "background") {
        setBackgroundSyncProgress(payload);
        if (payload.phase === "chunking" && generation === generationRef.current) {
          void refresh();
        }
      }
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refresh]);

  const updateConfig = useCallback(
    async (patch: Partial<ResolvedEmbeddingConfig>) => {
      if (!config) return;
      // Optimistic resolved view for the UI; persist as an explicit override
      // (every field pinned) so Settings edits keep winning over the bundled
      // preset until the user clears them.
      const next: ResolvedEmbeddingConfig = { ...config, ...patch };
      const override: EmbeddingProviderConfig = {
        kind: next.kind,
        remoteBaseUrl: next.remoteBaseUrl,
        remoteModel: next.remoteModel,
        remoteDimensions: next.remoteDimensions,
        remoteTrustedCertPem: next.remoteTrustedCertPem,
        remoteSystemId: next.remoteSystemId,
        remoteDisableTlsVerification: next.remoteDisableTlsVerification,
      };
      setConfigState(next);
      setBusy(true);
      try {
        await setEmbeddingConfig(override);
        setError(null);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
    },
    [config],
  );

  const saveApiKey = useCallback(async (apiKey: string) => {
    setBusy(true);
    try {
      await setEmbeddingRemoteApiKey(apiKey);
      setHasApiKey(true);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const downloadModel = useCallback(async () => {
    cancelRequestedRef.current = false;
    setModelStatus({ status: "downloading", progress: 0 });
    setBusy(true);
    try {
      await downloadEmbeddingModel();
      await refresh();
    } catch (e) {
      // A cancellation surfaces here as a rejection too (the backend
      // reports it as an error at the IPC layer) — the "cancelled" progress
      // event already reset `modelStatus`, so don't clobber it with an
      // "error" state for something the user asked for.
      if (cancelRequestedRef.current) return;
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      setModelStatus({ status: "error", message });
    } finally {
      setBusy(false);
    }
  }, [refresh]);

  /** Intentionally leaves `busy` untouched: the backend can't stop the
   * in-flight download, so starting a second attempt before this one's
   * `downloadModel()` promise actually settles would race the abandoned
   * attempt over the same on-disk cache file. `busy` only clears once the
   * real (possibly successful, possibly cancelled) result comes back. */
  const cancelDownload = useCallback(async () => {
    cancelRequestedRef.current = true;
    setModelStatus({ status: "notDownloaded" });
    try {
      await cancelEmbeddingModelDownload();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const sync = useCallback(async () => {
    const generation = generationRef.current;
    const root = repoRootRef.current;
    if (!root) return null;
    setBusy(true);
    setSyncProgress(null);
    const stillCurrent = () =>
      generation === generationRef.current && repoRootRef.current === root;
    try {
      const stats = await syncEmbeddings();
      if (!stillCurrent()) return null;
      const [nextIndexStatus, nextRepoIndexSummary] = await Promise.all([
        getEmbeddingIndexStatus(),
        getRepoIndexSummary(),
      ]);
      if (!stillCurrent()) return null;
      setLastSync(stats);
      setError(null);
      setIndexStatus(nextIndexStatus);
      setRepoIndexSummary(nextRepoIndexSummary);
      return stats;
    } catch (e) {
      if (!stillCurrent()) return null;
      setError(e instanceof Error ? e.message : String(e));
      return null;
    } finally {
      if (stillCurrent()) {
        setBusy(false);
        setSyncProgress(null);
      }
    }
  }, []);

  const providerConfigured =
    config?.kind === "local"
      ? modelStatus.status === "ready"
      : Boolean(config?.remoteBaseUrl && config?.remoteModel && hasApiKey);

  return {
    config,
    modelStatus,
    hasApiKey,
    busy,
    error,
    lastSync,
    indexStatus,
    repoIndexSummary,
    syncProgress,
    backgroundSyncProgress,
    providerConfigured,
    updateConfig,
    saveApiKey,
    downloadModel,
    cancelDownload,
    sync,
    refresh,
  };
}
