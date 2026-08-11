import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { GIT_PROGRESS_EVENT, type GitProgressEvent } from "../lib/git";

/** Subscribes to backend progress events for network-bound git operations
 * (fetch/pull/push/clone) — see `configure_transfer_progress`/
 * `configure_push_progress` in git_repo.rs and `progress_emitter` in
 * commands/git.rs, which throttles emission to ~10/sec. Call `reset()`
 * right before starting an operation so stale progress from a previous run
 * doesn't flash before the first real event arrives. */
export function useGitProgress() {
  const [event, setEvent] = useState<GitProgressEvent | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    listen<GitProgressEvent>(GIT_PROGRESS_EVENT, (e) => {
      if (!cancelled) setEvent(e.payload);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  const reset = useCallback(() => setEvent(null), []);

  return { event, reset };
}

/** Renders a progress event as a short label, e.g. "42%" or "1.2 МБ".
 * Returns null when there isn't enough information yet (no total known) —
 * callers should fall back to a static "in progress" label in that case. */
export function formatGitProgress(event: GitProgressEvent | null): string | null {
  if (!event) return null;
  if (event.kind === "transfer") {
    if (event.totalObjects > 0) {
      const pct = Math.round((event.receivedObjects / event.totalObjects) * 100);
      return `${pct}%`;
    }
    if (event.receivedBytes > 0) {
      return `${(event.receivedBytes / (1024 * 1024)).toFixed(1)} МБ`;
    }
    return null;
  }
  if (event.kind === "push") {
    if (event.total > 0) {
      const pct = Math.round((event.current / event.total) * 100);
      return `${pct}%`;
    }
    return null;
  }
  return null;
}
