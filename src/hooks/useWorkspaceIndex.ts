import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  buildIndex,
  clearIndex,
  getDiagnostics,
  INDEX_EVENT_CHANNEL,
  type Diagnostic,
  type IndexEvent,
  type IndexStats,
} from "../lib/workspaceIndex";

export type IndexStatus = "idle" | "building" | "ready" | "warning" | "error";

export type IndexProgress = {
  done: number;
  total: number;
  current: string;
};

type UseWorkspaceIndexOptions = {
  active?: boolean;
};

export function useWorkspaceIndex(
  repoRoot: string | null,
  options: UseWorkspaceIndexOptions = {},
) {
  const { active = true } = options;
  const [status, setStatus] = useState<IndexStatus>("idle");
  const [progress, setProgress] = useState<IndexProgress | null>(null);
  const [stats, setStats] = useState<IndexStats | null>(null);
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refreshDiagnostics = useCallback(async () => {
    try {
      setDiagnostics(await getDiagnostics());
    } catch {
      // Ignore — transient fetch failure; next event will retry.
    }
  }, []);

  const refreshDiagnosticsRef = useRef(refreshDiagnostics);
  refreshDiagnosticsRef.current = refreshDiagnostics;

  // Subscribe to index events.
  useEffect(() => {
    if (!active || !repoRoot) return;
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    listen<IndexEvent>(INDEX_EVENT_CHANNEL, (event) => {
      if (cancelled) return;
      const e = event.payload;
      switch (e.kind) {
        case "indexBuildingStarted":
          setStatus("building");
          setProgress(null);
          break;
        case "indexBuildingProgress":
          setStatus("building");
          setProgress(e.payload);
          break;
        case "indexBuildingFinished": {
          const s = e.payload.stats;
          setStats(s);
          setStatus(
            s.errors > 0 ? "error" : s.warnings > 0 ? "warning" : "ready",
          );
          setProgress(null);
          void refreshDiagnosticsRef.current();
          break;
        }
        case "indexUpdated":
        case "diagnosticsUpdated":
          void refreshDiagnosticsRef.current();
          break;
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, [active, repoRoot]);

  // Trigger build_index when a repo opens; clear on close.
  useEffect(() => {
    if (!active || !repoRoot) {
      setStatus("idle");
      setProgress(null);
      setStats(null);
      setDiagnostics([]);
      return;
    }
    setStatus("building");
    setError(null);
    let cancelled = false;
    buildIndex(repoRoot).catch((e) => {
      if (cancelled) return;
      setError(e instanceof Error ? e.message : String(e));
      setStatus("error");
    });
    return () => {
      cancelled = true;
      void clearIndex();
    };
  }, [active, repoRoot]);

  return { status, progress, stats, diagnostics, error, refreshDiagnostics };
}
