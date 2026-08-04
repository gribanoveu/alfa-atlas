import { useCallback, useEffect, useRef, useState } from "react";
import {
  cancelEmbeddingModelDownload,
  downloadEmbeddingModel,
  getEmbeddingConfig,
  getEmbeddingModelStatus,
  hasEmbeddingRemoteApiKey,
  listenModelDownloadProgress,
  setEmbeddingConfig,
  setEmbeddingRemoteApiKey,
  syncEmbeddings,
  type EmbeddingProviderConfig,
  type ModelStatus,
  type SyncStats,
} from "../lib/embeddings";

/**
 * Embedding provider config + local-model readiness + last sync result, in
 * one place — both `EmbeddingsTab` (Settings) and `AssistantPanel`
 * (RightDock checklist) call this so they read the same backend state via
 * the same logic, even though each holds its own React state instance
 * (the two are never visible at once — Settings is a modal overlay — so
 * there's no simultaneous-divergence case to guard against here).
 */
export function useEmbeddingSetup() {
  const [config, setConfigState] = useState<EmbeddingProviderConfig | null>(null);
  const [modelStatus, setModelStatus] = useState<ModelStatus>({ status: "notDownloaded" });
  const [hasApiKey, setHasApiKey] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastSync, setLastSync] = useState<SyncStats | null>(null);
  // The backend can't truly abort an in-flight download (see
  // `cancelEmbeddingModelDownload`'s doc comment) — this just tells
  // `downloadModel`'s catch block that a rejection is an expected
  // cancellation, not a real failure to surface as an error.
  const cancelRequestedRef = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const [nextConfig, nextStatus, nextHasKey] = await Promise.all([
        getEmbeddingConfig(),
        getEmbeddingModelStatus(),
        hasEmbeddingRemoteApiKey(),
      ]);
      setConfigState(nextConfig);
      setModelStatus(nextStatus);
      setHasApiKey(nextHasKey);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

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

  const updateConfig = useCallback(
    async (patch: Partial<EmbeddingProviderConfig>) => {
      if (!config) return;
      const next = { ...config, ...patch };
      setConfigState(next);
      setBusy(true);
      try {
        await setEmbeddingConfig(next);
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
    setBusy(true);
    try {
      const stats = await syncEmbeddings();
      setLastSync(stats);
      setError(null);
      return stats;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return null;
    } finally {
      setBusy(false);
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
    providerConfigured,
    updateConfig,
    saveApiKey,
    downloadModel,
    cancelDownload,
    sync,
    refresh,
  };
}
