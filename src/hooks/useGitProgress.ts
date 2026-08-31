import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import {
  GIT_PROGRESS_EVENT,
  type GitPhase,
  type GitProgressEvent,
} from "../lib/git";

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
 * callers should fall back to a static "in progress" label in that case.
 * For a button caption prefer `formatGitBusyLabel`, which decides whether the
 * detail can stand on its own. */
const PHASE_LABELS: Record<GitPhase, string> = {
  connecting: "Подключение…",
  authenticating: "Аутентификация…",
  hostKey: "Проверка ключа хоста…",
  remote: "Сервер…",
};

/** Caption for the button that started a network operation: "Клонирование…"
 * until there is something more specific to say, then the specific thing.
 *
 * Phase and checkout labels are full sentences of their own
 * ("Аутентификация…", "Распаковка 3/10"), so they *replace* the verb —
 * appending them produced "Клонирование… Аутентификация…", which reads as two
 * operations at once and stacks up two ellipses. Transfer progress is a bare
 * number with no words, so it keeps the verb: "Клонирование… 42%". */
export function formatGitBusyLabel(
  base: string,
  event: GitProgressEvent | null,
): string {
  const detail = formatGitProgress(event);
  if (!detail) return `${base}…`;
  const speaksForItself = event?.kind === "phase" || event?.kind === "checkout";
  return speaksForItself ? detail : `${base}… ${detail}`;
}

export function formatGitProgress(event: GitProgressEvent | null): string | null {
  if (!event) return null;
  if (event.kind === "phase") return PHASE_LABELS[event.phase];
  if (event.kind === "checkout") {
    // Checkout is where a clone that leaves only a `.git` directory is stuck,
    // so this label doubles as the diagnosis.
    return event.total > 0
      ? `Распаковка ${event.completed}/${event.total}`
      : "Распаковка…";
  }
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
